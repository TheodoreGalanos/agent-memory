// ABOUTME: Offline tests for the live walkthrough's connector and terminal formatting helpers.
// ABOUTME: No provider is contacted; the paid end-to-end run is a separate opt-in test.
import { describe, expect, it } from "vitest";
import type { Job } from "../../contracts/generated/host-response.js";
import type { WorkResult } from "../../contracts/generated/work-result.js";
import type { RenderManifest } from "../../contracts/generated/render-manifest.js";
import type { FormationResult } from "../../contracts/generated/formation-result.js";
import { input } from "../../packages/pi-worker/test/fixtures.js";
import {
  findingsToToolEvents,
  formatManifest,
  formatReply,
  summarizeFormation,
} from "../live-walkthrough-support.js";

const brief = input("before-correction").command.payload;
const memory = { memory_id: "0199a000-0000-7000-8000-000000000001", revision: 2, label: "W-101 fire resistance" };
const result: WorkResult = {
  schema_version: "1",
  status: "complete",
  examined_scope: brief.scope,
  inputs: { memories: [memory], sources: [], artifacts: [] },
  findings: [
    {
      statement: "The user corrected W-101 to 90 minutes.",
      origin: "observed",
      evidential_status: "attributed_statement",
      applicability: brief.scope,
      sources: [],
      supporting_memories: [memory],
      challenging_memories: [],
    },
    {
      statement: "No source has verified the 90 minute value.",
      origin: "agent_generated",
      evidential_status: "inference",
      applicability: brief.scope,
      sources: [],
      supporting_memories: [memory],
      challenging_memories: [],
    },
  ],
  coverage: { examined: ["Supplied memory W-101 fire resistance r2"], unexamined: ["No source file was available"] },
  unresolved_work: ["Verify against a source revision"],
  proposed_changes: [],
  child_outputs: [],
  known_effects: [],
  usage: { status: "known", input_tokens: 1200, output_tokens: 300, cost: null },
};
const job = {
  id: "0199a000-0000-7000-8000-00000000000a",
  root_id: "0199a000-0000-7000-8000-00000000000a",
  depth: 0,
  attempt: 1,
  session_id: "session",
  operation_id: "operation",
  state: "completed",
  deadline: "2026-09-18T10:00:00Z",
  cancel_requested: false,
  spec: { brief: { ...brief, purpose: "Report the W-101 rating" }, max_attempts: 2, retain_until: "2026-09-19T10:00:00Z" },
  result,
} as unknown as Job;

describe("findings become tool events", () => {
  const events = findingsToToolEvents(job, result, { provider: "azure-openai-responses", model: "gpt-4.1-mini" });
  it("keeps one event per finding with its own origin and evidential status", () => {
    expect(events).toHaveLength(2);
    expect(events.map((e) => e.event_id)).toEqual(["finding-1", "finding-2"]);
    expect(events[0].origin).toBe("observed");
    expect(events[0].evidential_status).toBe("attributed_statement");
    expect(events[1].origin).toBe("agent_generated");
    expect(events[1].evidential_status).toBe("inference");
    expect(new Set(events.map((e) => e.event_id)).size).toBe(2);
  });
  it("records the job, model and objective as conditions and carries coverage unchanged", () => {
    const content = events[0].content as Record<string, unknown>;
    expect(content.episode).toBe(job.id);
    expect(content.objective).toBe("Report the W-101 rating");
    expect(content.conditions).toEqual(expect.arrayContaining([expect.stringContaining(job.id), expect.stringContaining("gpt-4.1-mini")]));
    expect(content.coverage).toEqual(result.coverage);
    expect(content.explicit_contribution).toBe(false);
    expect(content.explicitly_selected).toBe(true);
    expect(content.actor_id).toBeNull();
    expect(content.retention).toBe("optional");
  });
  it("states the finding as knowledge and keeps the finding itself as evidence", () => {
    const content = events[1].content as { content: Record<string, unknown>; evidence: Record<string, unknown> };
    expect(content.content).toMatchObject({ family: "knowledge", statement: "No source has verified the 90 minute value." });
    expect(content.content.uncertainty).toEqual(result.coverage.unexamined);
    expect(content.evidence.finding).toEqual(result.findings[1]);
  });
});

describe("terminal formatting", () => {
  it("summarizes a manifest with its status, selection and payload", () => {
    const manifest = {
      status: "responded",
      lane: "main",
      provider: "azure-openai-responses",
      model: "gpt-4.1-mini",
      selected: [{ kind: "claim", text: "The user corrects W-101 to 90 minutes." }, { kind: "episode", text: "How the property was found" }],
      sources: [],
      conflicts: [],
      deferred: [{ entry_ids: ["x"], reason: "budget" }],
      final_payload_bytes: 5321,
      usage: { uncached_input_tokens: 1200, output_tokens: 300, cache_read_tokens: null, cache_write_tokens: null },
    } as unknown as RenderManifest;
    const line = formatManifest(manifest);
    expect(line).toContain("responded");
    expect(line).toContain("2 selected");
    expect(line).toContain("W-101 to 90 minutes");
    expect(line).toContain("1 deferred");
    expect(line).toContain("5321 bytes");
    expect(line).toContain("1200");
  });
  it("shows a reply's answers, latency and tokens without dumping the raw payload", () => {
    const provider = { id: "azure-reference", model: "gpt-4.1-mini" };
    const reply = { status: 200, raw: { answers: { "J01.retain": { type: "choice", choice: "retain" } }, output: "x".repeat(5000) }, usage: { input_tokens: 800, output_tokens: 20, cost_microunits: null } };
    const line = formatReply("reference", provider, reply, 1234);
    expect(line).toContain("J01.retain");
    expect(line).toContain("retain");
    expect(line).toContain("1234 ms");
    expect(line).toContain("800");
    expect(line.length).toBeLessThan(600);
  });
  it("summarizes retained, deferred and unresolved formation outcomes", () => {
    const formation = {
      records: [{ record: { label: "Finding: finding-1", evidential_status: "attributed_statement", qualification: { status: "candidate" }, content: { family: "knowledge" } }, reference: { revision: 1 } }],
      deferred: [{ event_id: "finding-2", reason: "Insufficient evidence", required: false }],
      unresolved: ["One contribution deferred", "One contribution deferred"],
      coverage: { examined: ["finding-1", "finding-2"], unexamined: [] },
      judgement_usage: { a: { input_tokens: 800, output_tokens: 20, cost_microunits: 320 }, b: { input_tokens: 900, output_tokens: 40, cost_microunits: null } },
    } as unknown as FormationResult;
    const text = summarizeFormation(formation);
    expect(text).toContain("Retained 1");
    expect(text).toContain("Finding: finding-1");
    expect(text).toContain("candidate");
    expect(text).toContain("finding-2: Insufficient evidence");
    expect(text.match(/One contribution deferred/g)).toHaveLength(1);
    expect(text).toContain("1700");
    expect(text).toContain("unknown");
  });
});

