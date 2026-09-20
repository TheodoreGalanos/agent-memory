// ABOUTME: Unit tests for the consolidation synthesis adapter: a scripted task model proposes descriptive
// ABOUTME: text, and every structural field the Host will re-check is derived from the window, not the model.
import { randomUUID } from "node:crypto";
import { expect, it } from "vitest";
import { BACKGROUND_CONTEXT as context } from "@earendil-works/pi-agent-core";
import { createModels, fauxProvider, fauxAssistantMessage } from "@earendil-works/pi-ai";
import type { ConsolidationWindow, MemoryVersion } from "../../../contracts/generated/consolidation-review.js";
import { synthesizeProposal } from "../src/adapters/synthesis.js";

const policy = { id: randomUUID(), revision: 1, label: "Test policy" };
const scope = { user_id: null, project_id: randomUUID(), task_id: randomUUID(), entity_ids: [], source_versions: [] };

function caseVersion(label: string, statement: string): MemoryVersion {
  return {
    version_id: randomUUID(),
    created_by: randomUUID(),
    decision_id: randomUUID(),
    recorded: { recorded_at: new Date().toISOString(), sequence: 1 },
    recorded_until: null,
    reference: { memory_id: randomUUID(), revision: 1, label },
    record: {
      label, scope, origin: "observed", evidential_status: "observation", availability: "routine",
      qualification: { status: "candidate" }, valid_time: { kind: "unknown" }, source_locators: [], derived_from: [],
      content: { family: "knowledge", statement, uncertainty: [], examined_coverage: [] },
      decision: { policy, action: "retain", reason: "test", constraints: [], required_evidence: [], expires_at: null },
    },
  };
}

function window(cases: MemoryVersion[]): ConsolidationWindow {
  return {
    id: randomUUID(), job_id: randomUUID(), policy, scope, cutoff: new Date().toISOString(),
    selection: { purpose: "Find the recurring cause", mechanism: "test", outcomes: [], source_context: "test", members: cases.map((c) => c.reference) },
    cases, exceptions: [], source_groups: [], unexamined: [], unknown_lineage: [],
  };
}

const synthesis = (overrides: Record<string, unknown> = {}) => ({
  label: "Method", concise_summary: "Summary", invariant: "Invariant", variations: [], untested: [],
  clause_text: "Clause", conditions: [], uncertainty: [], support_case_ids: [], is_procedure: false,
  purpose: "", applicability: [], exclusions: [], steps: [], evidence_criteria: [], decision_points: [], stopping_criteria: [], checks: [],
  ...overrides,
});

function taskModel(faux: ReturnType<typeof fauxProvider>) {
  const models = createModels();
  models.setProvider(faux.provider);
  return { models, model: faux.getModel(), maxPayloadBytes: 100_000 };
}

it("keeps only support case ids the model named that are actually in the window", async () => {
  const a = caseVersion("Case A", "The cache was stale"), b = caseVersion("Case B", "The cache was stale again");
  const w = window([a, b]);
  const faux = fauxProvider();
  faux.setResponses([fauxAssistantMessage(JSON.stringify(synthesis({ support_case_ids: [a.reference.memory_id, "not-in-window"] })))]);
  const proposal = await synthesizeProposal(taskModel(faux), w, context);
  expect(proposal.clauses).toHaveLength(1);
  expect(proposal.clauses[0].support).toEqual([a.reference]);
  expect(proposal.clauses[0].support.some((r) => r.memory_id === "not-in-window")).toBe(false);
});

it("grounds a clause in the whole cohort when the model names no case actually in the window", async () => {
  const a = caseVersion("Case A", "The cache was stale"), b = caseVersion("Case B", "The cache was stale again");
  const w = window([a, b]);
  const faux = fauxProvider();
  faux.setResponses([fauxAssistantMessage(JSON.stringify(synthesis({ support_case_ids: ["unrelated-id"] })))]);
  const proposal = await synthesizeProposal(taskModel(faux), w, context);
  expect(proposal.clauses[0].support.map((r) => r.memory_id).sort()).toEqual([a.reference.memory_id, b.reference.memory_id].sort());
});

it("derives support, exceptions, counterexamples and scope from the window rather than the model", async () => {
  const a = caseVersion("Case A", "The cache was stale");
  const w = { ...window([a]), exceptions: [{ memory_id: randomUUID(), revision: 1, label: "Exception" }] };
  const faux = fauxProvider();
  faux.setResponses([fauxAssistantMessage(JSON.stringify(synthesis({
    support_case_ids: [a.reference.memory_id], is_procedure: true, purpose: "Diagnose stale cache",
    applicability: ["Cache looks stale"], exclusions: [], steps: ["Clear the cache"], evidence_criteria: ["Cache is fresh"],
    decision_points: ["Is the cache stale?"], stopping_criteria: ["Cache is confirmed fresh"], checks: ["Cache age is checked"],
  })))]);
  const proposal = await synthesizeProposal(taskModel(faux), w, context);
  expect(proposal.scope).toEqual(w.scope);
  expect(proposal.clauses[0].exceptions).toEqual(w.exceptions);
  expect(proposal.procedure).toMatchObject({ family: "procedure", capabilities: ["read"], counterexamples: w.exceptions });
  expect(proposal.procedure?.family === "procedure" ? proposal.procedure.contract?.checks : []).toEqual(["Cache age is checked"]);
});

it("rejects a reply missing a required field rather than silently filling it in", async () => {
  const a = caseVersion("Case A", "The cache was stale");
  const w = window([a]);
  const faux = fauxProvider();
  const bad = synthesis() as Record<string, unknown>;
  delete bad.invariant;
  faux.setResponses([fauxAssistantMessage(JSON.stringify(bad))]);
  await expect(synthesizeProposal(taskModel(faux), w, context)).rejects.toThrow(/invariant/);
});

it("rejects a cohort payload that exceeds the task model's conservative allowance", async () => {
  const big = caseVersion("Case A", "x".repeat(5000));
  const w = window([big]);
  const faux = fauxProvider();
  faux.setResponses([fauxAssistantMessage(JSON.stringify(synthesis()))]);
  const deps = { ...taskModel(faux), maxPayloadBytes: 100 };
  await expect(synthesizeProposal(deps, w, context)).rejects.toThrow(/payload allowance/);
});
