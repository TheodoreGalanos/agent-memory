// ABOUTME: Runtime routing against a scripted Host: the primary provider decides, the fallback is consulted only
// ABOUTME: on primary failure, and comparison work is recorded after the decision without influencing it.
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { randomUUID } from "node:crypto";
import { BACKGROUND_CONTEXT as context } from "@earendil-works/pi-agent-core";
import { afterEach, beforeEach, expect, it } from "vitest";
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type { HostRequest } from "../../../contracts/generated/host-request.js";
import type { HostResponse } from "../../../contracts/generated/host-response.js";
import type { JudgementPacket } from "../../../contracts/generated/judgement-packet.js";
import type { SemanticAssessment } from "../../../contracts/generated/semantic-assessment.js";
import { openLocalSession } from "../../pi-worker/src/local-session.js";
import { input } from "../../pi-worker/test/fixtures.js";
import type { JudgementProvider, ProviderReply } from "../src/providers.js";
import { runJudgement } from "../src/runtime.js";
import { maximum, packet, reply } from "./fixtures.js";

const seconds = (n: number) => new Date(Date.now() + n * 1000).toISOString();

function assignmentFor(sessionId: string): Assignment {
  const brief = input("before-correction").command.payload;
  return {
    epoch: 1, owner_id: randomUUID(), expires_at: seconds(60),
    job: { id: randomUUID(), root_id: randomUUID(), depth: 0, attempt: 1, session_id: sessionId, operation_id: randomUUID(), state: "running", deadline: seconds(120), cancel_requested: false,
      spec: { brief, max_attempts: 1, retain_until: seconds(600) } },
  };
}

/** A Host that admits one packet and records assessments and decisions in memory. */
function scriptedHost(p: JudgementPacket) {
  const calls: HostRequest[] = [];
  const assessments = new Map<string, SemanticAssessment>();
  const host = {
    calls, assessments,
    async request(command: HostRequest): Promise<HostResponse> {
      calls.push(command);
      switch (command.action) {
        case "check_judgement": return { kind: "judgement_packet", packet: p };
        case "reserve": return { kind: "reservation", reservation: { id: randomUUID(), job_id: p.job_id, budget_id: p.budget_id, provider_attempt: command.provider_attempt, maximum: command.maximum, final_result: false, knowledge: "reserved" } };
        case "settle_usage": return { kind: "done", affected: 1 };
        case "publish_artifact": return { kind: "artifact", artifact: { id: command.request_id, label: command.label, bytes: 0, dependencies: [] } } as unknown as HostResponse;
        case "record_assessment": assessments.set(command.assessment.id, command.assessment); return { kind: "assessment", assessment: command.assessment };
        case "record_judgement_decision": return { kind: "judgement_decision", decision: command.decision };
        case "reuse_assessment": return { kind: "assessment", assessment: null };
        default: throw new Error(`Unscripted host command ${command.action}`);
      }
    },
  };
  return host;
}

/** A provider answering the fixture, or failing/hanging as scripted. */
function provider(id: string, script: { fail?: boolean; hangMs?: number; calls?: number } = {}): JudgementProvider & { calls: number } {
  const self = {
    id, model: `${id}-model`, release: `${id}-release`, distributions: false, maximum, calls: 0,
    async evaluate(p: JudgementPacket, signal: AbortSignal): Promise<ProviderReply> {
      self.calls++;
      if (script.hangMs) await new Promise<void>((done, fail) => { const t = setTimeout(done, script.hangMs); signal.addEventListener("abort", () => { clearTimeout(t); fail(new Error("aborted")); }, { once: true }); });
      if (script.fail) return { status: 503, raw: { failure: "down" }, usage: {} };
      return { status: 200, raw: { ...reply(p), model: `${id}-release` }, usage: { input_tokens: 10, output_tokens: 2 } };
    },
  };
  return self;
}

let base: string, local: Awaited<ReturnType<typeof openLocalSession>>, assignment: Assignment;
beforeEach(async () => {
  base = await mkdtemp(join(tmpdir(), "memory-judgement-"));
  local = await openLocalSession(base, "judgement-test");
  assignment = assignmentFor(local.session.metadata.id);
});
afterEach(async () => { await local.close(); await rm(base, { recursive: true, force: true }); });

const disclosureFor = (p: JudgementPacket, ...providers: string[]) => ({ policy: p.disclosure_policy, scope: p.scope, providers });
const withProviders = (p: JudgementPacket, ...providers: string[]): JudgementPacket => ({ ...p, allowed_providers: providers, questions: p.questions.map((q) => ({ ...q, allowed_providers: providers })) });

it("the primary provider alone drives the decision; a configured fallback is not consulted", async () => {
  const p = withProviders(packet("J02"), "jev", "azure");
  const host = scriptedHost(p);
  const primary = provider("jev"), fallback = provider("azure");
  const decision = await runJudgement(host, assignment, local.session, p, { primary, fallback, disclosure: disclosureFor(p, "jev", "azure") }, context);
  expect(decision.mode).toBe("primary");
  expect(decision.assessment_ids).toHaveLength(1);
  expect(host.assessments.get(decision.selected_assessment!)?.provider).toBe("jev");
  expect(primary.calls).toBe(1);
  expect(fallback.calls).toBe(0);
});

it("an unavailable primary falls back to the declared fallback within its own attempt window", async () => {
  const p = withProviders(packet("J02"), "jev", "azure");
  const host = scriptedHost(p);
  const primary = provider("jev", { hangMs: 5_000 }), fallback = provider("azure");
  const started = Date.now();
  const decision = await runJudgement(host, assignment, local.session, p, { primary, fallback, attemptTimeoutMs: 200, maxAttempts: 1, disclosure: disclosureFor(p, "jev", "azure") }, context);
  expect(Date.now() - started).toBeLessThan(2_000);
  expect(decision.mode).toBe("fallback");
  expect(decision.assessment_ids).toHaveLength(2);
  expect(host.assessments.get(decision.selected_assessment!)?.provider).toBe("azure");
  expect(host.assessments.get(decision.assessment_ids[0])?.status).toBe("unavailable");
});

it("without a fallback, an unavailable primary records an unresolved decision rather than guessing", async () => {
  const p = withProviders(packet("J02"), "jev");
  const host = scriptedHost(p);
  const decision = await runJudgement(host, assignment, local.session, p, { primary: provider("jev", { fail: true }), maxAttempts: 1, disclosure: disclosureFor(p, "jev") }, context);
  expect(decision.mode).toBe("unresolved");
  expect(decision.selected_assessment).toBeUndefined();
  expect(decision.decisions.J02.action).toBe("defer");
});

it("comparison work runs after the decision is recorded, on its own reservation, and cannot alter or block it", async () => {
  const p = withProviders(packet("J02"), "jev", "azure");
  const host = scriptedHost(p);
  const primary = provider("jev"), comparison = provider("azure", { fail: true });
  const decision = await runJudgement(host, assignment, local.session, p, { primary, comparison: { provider: comparison }, disclosure: disclosureFor(p, "jev", "azure") }, context);
  expect(decision.mode).toBe("primary");
  expect(decision.assessment_ids).toHaveLength(1);
  const recordedDecision = host.calls.findIndex((c) => c.action === "record_judgement_decision");
  const comparisonReserve = host.calls.findIndex((c, i) => i > recordedDecision && c.action === "reserve");
  expect(comparisonReserve).toBeGreaterThan(recordedDecision);
  const recorded = [...host.assessments.values()];
  expect(recorded.map((a) => a.provider).sort()).toEqual(["azure", "jev"]);
  expect(recorded.find((a) => a.provider === "azure")?.status).toBe("unavailable");
  // Recovery returns the same decision and repeats neither the primary nor the comparison call.
  const again = await runJudgement(host, assignment, local.session, p, { primary, comparison: { provider: comparison }, disclosure: disclosureFor(p, "jev", "azure") }, context);
  expect(again).toEqual(decision);
  expect(primary.calls).toBe(1);
  expect(comparison.calls).toBe(1);
});

it("rejects a primary and fallback sharing one identity, and a provider outside the disclosure grant", async () => {
  const p = withProviders(packet("J02"), "jev");
  const host = scriptedHost(p);
  await expect(runJudgement(host, assignment, local.session, p, { primary: provider("jev"), fallback: provider("jev"), disclosure: disclosureFor(p, "jev") }, context)).rejects.toThrow(/distinct/);
  await expect(runJudgement(host, assignment, local.session, p, { primary: provider("azure"), disclosure: disclosureFor(p, "azure") }, context)).rejects.toThrow(/disclosure/);
});
