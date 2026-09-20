// ABOUTME: Tests for the structural output validator: criteriaAdapter checks a settled result's own
// ABOUTME: coverage and findings against the brief's output criteria, and adapter selection by profile label.
import { expect, it } from "vitest";
import type { WorkBrief, WorkResult } from "../../../contracts/generated/assignment.js";
import { criteriaAdapter, selectAdapter, type TaskAdapter } from "../src/adapter.js";

const brief = (output_criteria: string[], profileLabel = "default"): WorkBrief =>
  ({ output_criteria, profile: { id: "p", revision: 1, label: profileLabel }, inputs: { memories: [], sources: [], artifacts: [] }, capabilities: { sources: [], queries: [], tools: ["read"] } }) as unknown as WorkBrief;

const result = (over: Partial<WorkResult>): WorkResult =>
  ({
    schema_version: "1", status: "complete", examined_scope: {}, inputs: { memories: [], sources: [], artifacts: [] },
    findings: [], coverage: { examined: [], unexamined: [] }, unresolved_work: [], proposed_changes: [], child_outputs: [],
    known_effects: [], usage: { status: "unknown" }, ...over,
  }) as unknown as WorkResult;

it("passes when every output criterion appears in the result's own examined coverage or a finding's statement", () => {
  const b = brief(["Clearance under beam B3 reported", "Missing coverage stated"]);
  const r = result({ coverage: { examined: ["Clearance under beam B3 reported"], unexamined: [] }, findings: [{ statement: "Missing coverage stated" } as never] });
  expect(criteriaAdapter.validate(b, r, [])).toEqual({ verdict: "passed", reasons: [] });
});

it("fails and names what is missing when an output criterion appears nowhere in coverage or findings", () => {
  const b = brief(["Clearance under beam B3 reported"]);
  const r = result({ coverage: { examined: ["Something unrelated"], unexamined: [] } });
  const validation = criteriaAdapter.validate(b, r, []);
  expect(validation.verdict).toBe("failed");
  expect(validation.reasons.join(" ")).toContain("Clearance under beam B3 reported");
});

it("fails a complete status that still leaves unresolved work, even with every criterion covered", () => {
  const b = brief(["Covered"]);
  const r = result({ status: "complete", coverage: { examined: ["Covered"], unexamined: [] }, unresolved_work: ["Night shift measurements are unavailable"] });
  const validation = criteriaAdapter.validate(b, r, []);
  expect(validation.verdict).toBe("failed");
  expect(validation.reasons.join(" ")).toContain("Night shift measurements are unavailable");
});

it("cannot assess a blocked task either way", () => {
  const b = brief(["Covered"]);
  const r = result({ status: "blocked" });
  expect(criteriaAdapter.validate(b, r, [])).toEqual({ verdict: "unknown", reasons: expect.arrayContaining([expect.stringContaining("blocked")]) });
});

it("selects an installed adapter by the brief's profile label, and the default otherwise", () => {
  const custom: TaskAdapter = { ...criteriaAdapter, id: "custom-adapter" };
  expect(selectAdapter(brief([], "custom-adapter"), [custom])).toBe(custom);
  expect(selectAdapter(brief([], "unrelated-label"), [custom])).toBe(criteriaAdapter);
  expect(selectAdapter(brief([], "default"))).toBe(criteriaAdapter);
});
