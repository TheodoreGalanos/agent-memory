// ABOUTME: Tests for the `decide` application tool: pure candidate-set construction and hashing, the
// ABOUTME: speculative-branch planner and reactive hint as standalone functions, and a real-Host round trip.
import { spawn } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { once } from "node:events";
import { createServer } from "node:net";
import { randomBytes, randomUUID } from "node:crypto";
import { BACKGROUND_CONTEXT as context } from "@earendil-works/pi-agent-core";
import { createModels, fauxProvider, fauxAssistantMessage, fauxToolCall } from "@earendil-works/pi-ai";
import { expect, it, describe } from "vitest";
import type { HostRequest } from "../../../contracts/generated/host-request.js";
import type { HostResponse } from "../../../contracts/generated/host-response.js";
import type { JudgementPacket } from "../../../contracts/generated/judgement-packet.js";
import { HostClient } from "../../host-client/src/client.js";
import { input } from "../../pi-worker/test/fixtures.js";
import { openLocalSession } from "../../pi-worker/src/local-session.js";
import { GenerativeProvider, type JudgementProvider } from "../../judgement/src/providers.js";
import { maximum } from "../../judgement/test/fixtures.js";
import { decide } from "../../judgement/src/policy.js";
import { catalogueResponses } from "../../judgement/src/responses.js";
import { fenceFor } from "../../host-client/src/fence.js";
import { composeApplication, type Services } from "../src/composition.js";
import { investigationModule } from "../src/investigation.js";
import { runSupervisor } from "../src/index.js";
import {
  OPERATION_KINDS,
  candidateSetId,
  checkResultCoverage,
  decisionModule,
  eligibleOperations,
  planDecisionBranch,
  proposeTaskCheck,
  reactiveDecisionHint,
  type ActivatedObservation,
  type CandidateOption,
  type DecisionObservations,
} from "../src/decision.js";

const emptyState = (over: Partial<DecisionObservations> = {}): DecisionObservations => ({
  workspace_summary: "Nothing outstanding.",
  change_cursor: 0,
  recent_tool_results: [],
  activated: [],
  unresolved_work: [],
  ...over,
});

describe("eligibleOperations", () => {
  it("always offers continue, inspect_further and return_unresolved, bounded to 12 candidates", () => {
    const many: ActivatedObservation[] = Array.from({ length: 20 }, (_, i) => ({
      memory_id: randomUUID(),
      revision: 1,
      family: "procedure",
      disposition: "applicable",
      conflict_group: `group-${i % 2}`,
    }));
    const options = eligibleOperations(emptyState({ activated: many }), ["read"]);
    expect(options.length).toBeLessThanOrEqual(12);
    expect(options.map((o) => o.operation)).toContain("continue");
    expect(options.map((o) => o.operation)).toContain("inspect_further");
    expect(options.map((o) => o.operation)).toContain("return_unresolved");
    for (const option of options) expect(OPERATION_KINDS).toContain(option.operation);
  });

  it("is deterministic for identical observations", () => {
    const state = emptyState({ unresolved_work: ["check drainage"] });
    expect(eligibleOperations(state, ["read"])).toEqual(eligibleOperations(state, ["read"]));
  });

  it("offers apply_procedure only for applicable or candidate_method procedure dispositions, and only when a tool is granted", () => {
    const base: ActivatedObservation = { memory_id: randomUUID(), revision: 3, family: "procedure", disposition: "applicable" };
    const applicable = eligibleOperations(emptyState({ activated: [base] }), ["read"]);
    expect(applicable.map((o) => o.operation)).toContain("apply_procedure");

    const candidateMethod = eligibleOperations(emptyState({ activated: [{ ...base, disposition: "candidate_method" }] }), ["read"]);
    expect(candidateMethod.map((o) => o.operation)).toContain("apply_procedure");

    const needsInvestigation = eligibleOperations(emptyState({ activated: [{ ...base, disposition: "needs_investigation" }] }), ["read"]);
    expect(needsInvestigation.map((o) => o.operation)).not.toContain("apply_procedure");

    const knowledge = eligibleOperations(emptyState({ activated: [{ ...base, family: "knowledge", disposition: "applicable" }] }), ["read"]);
    expect(knowledge.map((o) => o.operation)).not.toContain("apply_procedure");

    const noTools = eligibleOperations(emptyState({ activated: [base] }), []);
    expect(noTools.map((o) => o.operation)).not.toContain("apply_procedure");
  });

  it("offers investigate_conflict only when two or more activated candidates share a conflict group", () => {
    const one = eligibleOperations(
      emptyState({ activated: [{ memory_id: randomUUID(), revision: 1, family: "knowledge", disposition: "applicable", conflict_group: "g" }] }),
      ["read"],
    );
    expect(one.map((o) => o.operation)).not.toContain("investigate_conflict");
    const two = eligibleOperations(
      emptyState({
        activated: [
          { memory_id: randomUUID(), revision: 1, family: "knowledge", disposition: "applicable", conflict_group: "g" },
          { memory_id: randomUUID(), revision: 1, family: "knowledge", disposition: "applicable", conflict_group: "g" },
        ],
      }),
      ["read"],
    );
    expect(two.map((o) => o.operation)).toContain("investigate_conflict");
  });
});

describe("candidateSetId", () => {
  it("changes when the candidate set or the change cursor changes; stable otherwise", () => {
    const a: CandidateOption[] = [{ operation: "continue", label: "x", target: { kind: "none" }, preconditions: [], cost_class: "none" }];
    const b: CandidateOption[] = [{ operation: "continue", label: "y", target: { kind: "none" }, preconditions: [], cost_class: "none" }];
    expect(candidateSetId(a, 0)).toBe(candidateSetId(a, 0));
    expect(candidateSetId(a, 0)).not.toBe(candidateSetId(a, 1));
    expect(candidateSetId(a, 0)).not.toBe(candidateSetId(b, 0));
  });
});

describe("planDecisionBranch", () => {
  it("batches the J31 choice with a J06 relevance check into one packet when a procedure candidate exists", () => {
    const options = eligibleOperations(
      emptyState({ activated: [{ memory_id: randomUUID(), revision: 1, family: "procedure", disposition: "applicable" }] }),
      ["read"],
    );
    expect(options.map((o) => o.operation)).toContain("apply_procedure");
    const scope = { user_id: null, project_id: null, task_id: null, entity_ids: [], source_versions: [] };
    const waves = planDecisionBranch(randomUUID(), options, scope, ["jev"], new Date().toISOString(), new Date(Date.now() + 60_000).toISOString());
    expect(waves).toHaveLength(1);
    expect(waves[0]).toHaveLength(1);
    expect(waves[0][0].questions.map((q) => q.definition.id)).toEqual(["J31", "J06"]);
  });

  it("plans only the J31 question when no procedure candidate exists", () => {
    const options = eligibleOperations(emptyState(), ["read"]);
    const scope = { user_id: null, project_id: null, task_id: null, entity_ids: [], source_versions: [] };
    const waves = planDecisionBranch(randomUUID(), options, scope, ["jev"], new Date().toISOString(), new Date(Date.now() + 60_000).toISOString());
    expect(waves.flat().flatMap((p) => p.questions.map((q) => q.definition.id))).toEqual(["J31"]);
  });
});

describe("reactiveDecisionHint", () => {
  const hint = reactiveDecisionHint(2);
  it("adds a hint after two consecutive failing tool results", async () => {
    const messages = [
      { role: "toolResult", toolName: "bash", isError: true },
      { role: "toolResult", toolName: "bash", isError: true },
    ];
    const result = await hint({ messages, systemPrompt: "base" });
    expect(result?.systemPrompt).toContain("decide");
  });
  it("adds a hint after two identical repeated tool calls", async () => {
    const messages = [
      { role: "assistant", content: [{ type: "toolCall", name: "bash", arguments: { cmd: "ls" } }] },
      { role: "assistant", content: [{ type: "toolCall", name: "bash", arguments: { cmd: "ls" } }] },
    ];
    const result = await hint({ messages, systemPrompt: "base" });
    expect(result?.systemPrompt).toContain("decide");
  });
  it("does nothing for a single failure or non-identical calls", async () => {
    const oneFailure = [{ role: "toolResult", toolName: "bash", isError: true }];
    expect(await hint({ messages: oneFailure, systemPrompt: "base" })).toBeUndefined();
    const different = [
      { role: "assistant", content: [{ type: "toolCall", name: "bash", arguments: { cmd: "ls" } }] },
      { role: "assistant", content: [{ type: "toolCall", name: "read", arguments: { path: "a" } }] },
    ];
    expect(await hint({ messages: different, systemPrompt: "base" })).toBeUndefined();
  });
});

// --- Real-Host integration ---

function expectKind<K extends HostResponse["kind"]>(response: HostResponse, kind: K): Extract<HostResponse, { kind: K }> {
  if (response.kind !== kind) throw new Error(`Expected ${kind}, got ${response.kind}: ${JSON.stringify(response).slice(0, 300)}`);
  return response as Extract<HostResponse, { kind: K }>;
}
const minutes = (n: number) => new Date(Date.now() + n * 60_000).toISOString();
async function freePort(): Promise<number> {
  const socket = createServer();
  socket.listen(0, "127.0.0.1");
  await once(socket, "listening");
  const address = socket.address();
  if (!address || typeof address === "string") throw new Error("No port");
  await new Promise<void>((done, fail) => socket.close((e) => (e ? fail(e) : done())));
  return address.port;
}
const taskService = (models: ReturnType<typeof createModels>, faux: ReturnType<typeof fauxProvider>) =>
  ({ models, model: faux.getModel(), maxPayloadBytes: 100_000 });

interface World {
  base: string; url: string; host: ReturnType<typeof spawn>; client: HostClient; pool: HostClient;
  scope: import("../../../contracts/generated/host-request.js").Scope; budget: { id: string };
  submit: (extra: Record<string, unknown>) => Promise<import("../../../contracts/generated/host-response.js").Job>;
  job: (id: string) => Promise<import("../../../contracts/generated/host-response.js").Job>;
  close: () => Promise<void>;
}
async function world(): Promise<World> {
  const base = await mkdtemp(join(tmpdir(), "memory-decision-"));
  const port = await freePort();
  const tenant_id = randomUUID(), actor_id = randomUUID(), project_id = randomUUID();
  const scope = { user_id: null, project_id, task_id: null, entity_ids: [], source_versions: [] };
  const tokens = { client: randomBytes(32).toString("hex"), admin: randomBytes(32).toString("hex"), pool: randomBytes(32).toString("hex") };
  const expires_at = minutes(60);
  await writeFile(join(base, "host.json"), JSON.stringify({
    listen: `127.0.0.1:${port}`, database_url: `sqlite://${join(base, "memory.db")}?mode=rwc`, artifact_root: join(base, "artifacts"),
    credentials: [
      { token: tokens.client, tenant_id, actor_id, scope, role: { kind: "client" }, expires_at },
      { token: tokens.admin, tenant_id, actor_id: randomUUID(), scope: { ...scope, project_id: null }, role: { kind: "administrator" }, expires_at },
      { token: tokens.pool, tenant_id, actor_id: randomUUID(), scope: { ...scope, project_id: null }, role: { kind: "pool", classes: ["interactive", "deferred"] }, expires_at },
    ],
  }), { mode: 0o600 });
  const host = spawn(resolve("target/debug/memory-host"), [join(base, "host.json")], { stdio: ["ignore", "pipe", "pipe"] });
  let errors = ""; host.stderr!.on("data", (c) => (errors += c));
  await Promise.race([once(host.stdout!, "data"), once(host, "exit").then(() => { throw new Error(errors); })]);
  const url = `http://127.0.0.1:${port}`;
  const client = new HostClient(url, tokens.client), admin = new HostClient(url, tokens.admin);
  const fixture = input("before-correction").command;
  const created = expectKind(await admin.request({ action: "user", request: { action: "mutate", request_id: randomUUID(), mutation: {
    action: "create_policy", label: "Decision test", scope, effective: { kind: "unknown" },
    policy: { retention_purpose: "test", allowed_uses: ["task_context"], source_rules: [], evidence_requirements: [], applicability_rules: [], budget_class: "test", scheduling_priority: 0, judgement_dispositions: [], notification_policy: "material", qualification_requirements: [] } } } }), "user").result;
  if (created.kind !== "policy") throw new Error("policy");
  const budget = { id: randomUUID(), scope, limit: { tokens: 5_000_000, provider_calls: 50, output_bytes: 5_000_000, cost_microunits: 5_000_000, sandbox_cpu_ms: 0, sandbox_time_ms: 0 }, final_result_reserve: { tokens: 100, provider_calls: 0, output_bytes: 1000, cost_microunits: 0, sandbox_cpu_ms: 0, sandbox_time_ms: 0 }, deadline: minutes(30), max_child_depth: 0, max_child_concurrency: 0, pricing_revision: "test" };
  await admin.request({ action: "create_budget", budget });
  return {
    base, url, host, client, pool: new HostClient(url, tokens.pool), scope, budget,
    async submit(extra) {
      const task_id = randomUUID();
      const brief = { ...fixture.payload, process: "investigation", task_id, scope: { ...scope, task_id }, policy: created.policy.reference, purpose: "decision test",
        inputs: { memories: [], sources: [], artifacts: [] }, capabilities: { sources: [], queries: [], tools: ["decide"] }, known_conflicts: [], evidence_cutoff: new Date().toISOString(),
        profile: { id: randomUUID(), revision: 1, label: "investigation profile" },
        limits: { root_budget_id: budget.id, max_provider_attempts: 8, max_tokens: 4_000_000, max_output_bytes: 1_000_000, max_child_depth: 0, max_child_concurrency: 0 }, ...extra };
      const command = { ...fixture, request_id: randomUUID(), tenant_id, actor_id, scope: brief.scope, budget_id: budget.id, deadline: minutes(20), payload: { brief, parent_id: null, max_attempts: 2, retain_until: minutes(1440) } };
      return expectKind(await client.request({ action: "submit", command } as HostRequest), "job").job;
    },
    async job(id) { return expectKind(await client.request({ action: "inspect_job", job_id: id }), "job").job; },
    async close() { host.kill("SIGINT"); await once(host, "exit"); await rm(base, { recursive: true, force: true }); },
  };
}
function scriptedNext(faux: ReturnType<typeof fauxProvider>, models: ReturnType<typeof createModels>, answers: string[]): JudgementProvider {
  const generative = new GenerativeProvider("reference", models, faux.getModel(), maximum, 2048, faux.getModel().id);
  let call = 0;
  return {
    id: generative.id, model: generative.model, release: generative.release, maximum, distributions: false,
    async evaluate(packet: JudgementPacket, signal: AbortSignal) {
      const choice = answers[Math.min(call, answers.length - 1)];
      call++;
      const answers_ = Object.fromEntries(
        packet.questions.flatMap((q) =>
          q.definition.questions.map((question) => [
            `${q.definition.id}.${question.key}`,
            { type: "choice", choice: q.definition.id === "J31" ? choice : "no" },
          ]),
        ),
      );
      faux.setResponses([fauxAssistantMessage(JSON.stringify({ answers: answers_ }))]);
      return generative.evaluate(packet, signal);
    },
  };
}
const SETTLED = ["completed", "partial", "failed"];

it(
  "a task granted `decide` gets a Host-checked next operation from a bounded menu; two calls take two Jev picks",
  async () => {
    const w = await world();
    try {
      const job = await w.submit({});
      const faux = fauxProvider(), models = createModels();
      models.setProvider(faux.provider);
      const judgeFaux = fauxProvider(), judgeModels = createModels();
      judgeModels.setProvider(judgeFaux.provider);
      const reference = scriptedNext(judgeFaux, judgeModels, ["continue", "inspect_further"]);
      const state1 = { state: { workspace_summary: "Clearance measured at 2.0 m; nothing outstanding.", change_cursor: 0, recent_tool_results: [], activated: [], unresolved_work: [] } };
      const state2 = { state: { workspace_summary: "A read failed twice.", change_cursor: 1, recent_tool_results: [{ tool: "read", is_error: true, summary: "boom" }], activated: [], unresolved_work: ["check drainage plan"] } };
      const finalResult = { schema_version: "1", status: "complete", examined_scope: job.spec.brief.scope, inputs: job.spec.brief.inputs, findings: [], coverage: { examined: ["Scripted"], unexamined: [] }, unresolved_work: [], proposed_changes: [], child_outputs: [], known_effects: [], usage: { status: "unknown", input_tokens: null, output_tokens: null, cost: null } };
      faux.setResponses([
        fauxAssistantMessage(fauxToolCall("decide", state1), { stopReason: "toolUse" }),
        (transcript) => {
          const result = transcript.messages.find((m) => m.role === "toolResult");
          expect(result?.isError).toBe(false);
          const text = result?.content.find((c) => c.type === "text")?.text ?? "";
          expect(JSON.parse(text).next).toBe("continue");
          return fauxAssistantMessage(fauxToolCall("decide", state2), { stopReason: "toolUse" });
        },
        (transcript) => {
          const results = transcript.messages.filter((m) => m.role === "toolResult");
          const last = results[results.length - 1];
          expect(last?.isError).toBe(false);
          const text = last?.content.find((c) => c.type === "text")?.text ?? "";
          expect(JSON.parse(text).next).toBe("inspect_further");
          return fauxAssistantMessage(JSON.stringify(finalResult));
        },
      ]);
      const composition = composeApplication(
        { taskModel: taskService(models, faux), judgement: { primary: reference, maximum } } as unknown as Services,
        [investigationModule, decisionModule],
      );
      await runSupervisor({
        host: w.pool, hostUrl: w.url, concurrency: 1, pollMs: 50, leaseSeconds: 60, sessionsDirectory: join(w.base, "sessions"),
        composition, log: () => {}, until: async () => SETTLED.includes((await w.job(job.id)).state),
      }, context);
      const done = await w.job(job.id);
      expect(done.state).toBe("completed");
    } finally {
      await w.close();
    }
  },
  120_000,
);

it("check_decision rejects a stale target and accepts a fresh one", async () => {
  const w = await world();
  try {
    const job = await w.submit({});
    const claimed = await w.pool.request({ action: "claim_next", processes: ["investigation"], lease_seconds: 60 });
    if (claimed.kind !== "work") throw new Error("expected work");
    const worker = new HostClient(w.url, claimed.credential.token);
    const fence = fenceFor(claimed.assignment);
    await worker.request({ action: "start", fence, composition: null });
    const draft = { label: "Clearance", scope: job.spec.brief.scope, content: { family: "knowledge", statement: "2.0 m", subject: null, predicate: null, uncertainty: [], examined_coverage: [] }, origin: "observed", evidential_status: "attributed_statement", availability: "routine", qualification: { status: "candidate" }, valid_time: { kind: "unknown" }, source_locators: [], derived_from: [], decision: { policy: { id: randomUUID(), label: "x", revision: 1 }, action: "retain", reason: "test", constraints: [], required_evidence: [], expires_at: null } };
    const policyRef = job.spec.brief.policy;
    const contributed = expectKind(await w.client.request({ action: "user", request: { action: "mutate", request_id: randomUUID(), mutation: { action: "contribute", record: { ...draft, decision: { ...draft.decision, policy: policyRef } } } } } as HostRequest), "user").result;
    if (contributed.kind !== "memory") throw new Error("memory");
    const reference = contributed.memory.reference;
    const fresh = expectKind(
      await worker.request({ action: "check_decision", fence, candidate_set_id: "forged-id", choice: { operation: "inspect_further", target: { kind: "memory", reference } } }),
      "decision_check",
    );
    expect(fresh.accepted).toBe(true);
    await w.client.request({ action: "user", request: { action: "mutate", request_id: randomUUID(), mutation: { action: "correct", expected: reference, record: { ...draft, decision: { ...draft.decision, policy: policyRef, action: "revise" }, content: { family: "knowledge", statement: "2.4 m", subject: null, predicate: null, uncertainty: [], examined_coverage: [] } } } } } as HostRequest);
    const stale = expectKind(
      await worker.request({ action: "check_decision", fence, candidate_set_id: "forged-id", choice: { operation: "inspect_further", target: { kind: "memory", reference } } }),
      "decision_check",
    );
    expect(stale.accepted).toBe(false);
    expect(stale.reasons).toContain("stale");
  } finally {
    await w.close();
  }
}, 60_000);

it("check_decision rejects an operation the budget has no headroom for", async () => {
  const w = await world();
  try {
    const job = await w.submit({ limits: { root_budget_id: w.budget.id, max_provider_attempts: 8, max_tokens: 5_000_000, max_output_bytes: 1_000_000, max_child_depth: 0, max_child_concurrency: 0 } });
    const claimed = await w.pool.request({ action: "claim_next", processes: ["investigation"], lease_seconds: 60 });
    if (claimed.kind !== "work") throw new Error("expected work");
    const worker = new HostClient(w.url, claimed.credential.token);
    const fence = fenceFor(claimed.assignment);
    await worker.request({ action: "start", fence, composition: null });
    await worker.request({
      action: "reserve", fence, provider_attempt: "consume-budget",
      maximum: { tokens: 4_999_880, cost_microunits: 0, provider_calls: 1, sandbox_cpu_ms: 0, sandbox_time_ms: 0, output_bytes: 0 },
      final_result: false,
    });
    const rejected = expectKind(
      await worker.request({ action: "check_decision", fence, candidate_set_id: "x", choice: { operation: "investigate_conflict", target: { kind: "none" } } }),
      "decision_check",
    );
    expect(rejected.accepted).toBe(false);
    expect(rejected.reasons).toContain("budget_exceeded");
  } finally {
    await w.close();
  }
}, 60_000);

it("checkResultCoverage appends an unresolved line only on a revise-class J21 answer", async () => {
  const w = await world();
  try {
    const job = await w.submit({});
    const claimed = await w.pool.request({ action: "claim_next", processes: ["investigation"], lease_seconds: 60 });
    if (claimed.kind !== "work") throw new Error("expected work");
    const worker = new HostClient(w.url, claimed.credential.token);
    const fence = fenceFor(claimed.assignment);
    await worker.request({ action: "start", fence, composition: null });
    const local = await openLocalSession(join(w.base, "coverage-sessions"), claimed.assignment.job.session_id);
    try {
      const faux = fauxProvider(), models = createModels();
      models.setProvider(faux.provider);
      const disclosure = { policy: claimed.assignment.job.spec.brief.disclosure_policy, scope: claimed.assignment.job.spec.brief.scope, providers: ["reference"] };
      const result = { schema_version: "1" as const, status: "complete" as const, examined_scope: claimed.assignment.job.spec.brief.scope, inputs: claimed.assignment.job.spec.brief.inputs, findings: [], coverage: { examined: ["Clearance measured at 2.0 m"], unexamined: [] }, unresolved_work: [], proposed_changes: [], child_outputs: [], known_effects: [], usage: { status: "unknown" as const, input_tokens: null, output_tokens: null, cost: null } };
      const overclaiming = new GenerativeProvider("reference", models, faux.getModel(), maximum, 2048, faux.getModel().id);
      faux.setResponses([fauxAssistantMessage(JSON.stringify({ answers: { "J21.assessment": { type: "choice", choice: "overclaimed" } } }))]);
      const revised = await checkResultCoverage(
        worker, claimed.assignment, local.session, result, [{ kind: "tool_call", content: { tool: "read" } }],
        { primary: overclaiming, disclosure, maxAttempts: 1, responses: catalogueResponses },
        context,
      );
      expect(revised.unresolved_work).toHaveLength(1);
      expect(revised.unresolved_work[0]).toContain("Coverage check");

      faux.setResponses([fauxAssistantMessage(JSON.stringify({ answers: { "J21.assessment": { type: "choice", choice: "justified" } } }))]);
      const justified = await checkResultCoverage(
        worker, claimed.assignment, local.session, { ...result, unresolved_work: [] }, [{ kind: "tool_call", content: { tool: "read" } }],
        { primary: overclaiming, disclosure, maxAttempts: 1, responses: catalogueResponses },
        context,
      );
      expect(justified.unresolved_work).toEqual([]);
    } finally {
      await local.close();
    }
  } finally {
    await w.close();
  }
}, 60_000);

it("proposeTaskCheck registers a task-local check whose packets are limited to investigate/retrieve_further/defer", async () => {
  const w = await world();
  try {
    const job = await w.submit({});
    const claimed = await w.pool.request({ action: "claim_next", processes: ["investigation"], lease_seconds: 60 });
    if (claimed.kind !== "work") throw new Error("expected work");
    const worker = new HostClient(w.url, claimed.credential.token);
    const fence = fenceFor(claimed.assignment);
    await worker.request({ action: "start", fence, composition: null });
    const check = await proposeTaskCheck(worker, claimed.assignment, { id: randomUUID(), family: "J17", expires_in_seconds: 60 }, ["reference"]);
    expect(check.definition.permitted_uses).toEqual(["investigation"]);
    // Regardless of the reused family's own response rules, a packet bound to a local check is
    // always restricted to investigate/retrieve_further/defer.
    const scope = claimed.assignment.job.spec.brief.scope;
    const packet: JudgementPacket = {
      id: randomUUID(), job_id: claimed.assignment.job.id, subject: "test", frame: "test", scope,
      inputs: { sources: [], memories: [], artifacts: [] }, evidence: [], missing: [],
      questions: [{ definition: check.definition, disclosure_scope: scope, allowed_providers: ["reference"], depends_on: [] }],
      disclosure_policy: claimed.assignment.job.spec.brief.disclosure_policy, allowed_providers: ["reference"],
      policy: claimed.assignment.job.spec.brief.policy, budget_id: claimed.assignment.job.spec.brief.limits.root_budget_id,
      evidence_cutoff: new Date().toISOString(), fresh_after: new Date(Date.now() - 60_000).toISOString(),
      deadline: check.expires_at, expires_at: check.expires_at, local_check_id: check.id,
    };
    const assessment = { id: randomUUID(), packet_id: packet.id, raw_artifact_id: randomUUID(), reservation_id: randomUUID(), provider: "reference", requested_model: "test", status: "answered" as const, answers: { [`${check.definition.id}.${check.definition.questions[0].key}`]: { type: "choice" as const, choice: "not_satisfied" } }, usage: { input_tokens: 1, output_tokens: 1 }, assessed_at: new Date().toISOString(), expires_at: packet.expires_at };
    const outcome = decide(packet, randomUUID(), [assessment], assessment, "primary", catalogueResponses);
    expect(["investigate", "retrieve_further", "defer"]).toContain(outcome.decisions[check.definition.id].action);
    await worker.request({ action: "retire_task_check", fence, id: check.id });
  } finally {
    await w.close();
  }
}, 60_000);
