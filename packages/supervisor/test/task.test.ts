// ABOUTME: Tests for pool task helpers: input memory delivery into the workspace, the WorkResult
// ABOUTME: template given to the model, and the strict provider output schema.
import { describe, expect, it } from "vitest";
import { Ajv2020 } from "ajv/dist/2020.js";
import type { WorkResult } from "../../../contracts/generated/work-result.js";
import type { WorkspaceState } from "../../../contracts/generated/workspace-state.js";
import type { MemoryVersion } from "../../../contracts/generated/host-response.js";
import { workspaceFromBrief } from "../../pi-worker/src/workspace.js";
import { input } from "../../pi-worker/test/fixtures.js";
import { deliverInputMemories, workResultOutputSchema, workResultTemplate } from "../src/task.js";

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

describe("work result template", () => {
  it("fixes scope and inputs from the brief and leaves only the model's parts open", () => {
    const template = workResultTemplate({ ...brief, inputs: { memories: [memory], sources: [], artifacts: [] } });
    expect(template.examined_scope).toEqual(brief.scope);
    expect(template.inputs.memories).toEqual([memory]);
    expect(template.findings).toEqual([]);
    // The Host refuses complete results with unexamined coverage or unresolved work.
    expect(template.status).toBe("partial");
    expect(template.usage).toEqual({ status: "unknown", input_tokens: null, output_tokens: null, cost: null });
    expect(template.known_effects).toEqual([]);
  });
});

describe("input memory delivery", () => {
  const granted = { ...brief, inputs: { memories: [memory], sources: [], artifacts: [] } };
  const workspace: WorkspaceState = workspaceFromBrief(granted, "2026-09-18T12:00:00Z", "2026-09-19T12:00:00Z");
  const version = {
    reference: memory,
    record: {
      label: "W-101 fire resistance",
      scope: brief.scope,
      origin: "observed",
      evidential_status: "attributed_statement",
      availability: "routine",
      qualification: { status: "candidate" },
      valid_time: { kind: "unknown" },
      content: { family: "knowledge", statement: "The user corrects W-101 to 90 minutes.", subject: null, predicate: null, uncertainty: [], examined_coverage: [] },
    },
  } as unknown as MemoryVersion;
  it("replaces the label-only entry with the record's content and status", () => {
    const before = workspace.entries.find((e) => e.kind === "memory")!;
    expect(before.text).toBe("W-101 fire resistance");
    expect(before.exposure).toBe("not_read");
    const delivered = deliverInputMemories(workspace, [version]);
    const after = delivered.entries.find((e) => e.kind === "memory")!;
    expect(after.id).toBe(before.id);
    expect(after.text).toContain("W-101 to 90 minutes");
    expect(after.text).toContain("candidate");
    expect(after.origin).toBe("observed");
    expect(after.evidential_status).toBe("attributed_statement");
    expect(after.exposure).toBe("parent_read");
    expect(after.decision_relevant).toBe(true);
    expect(after.inputs.memories).toEqual([memory]);
    expect(workspace.entries.find((e) => e.kind === "memory")!.text).toBe("W-101 fire resistance");
  });
  it("refuses a record at a different revision or one that was not granted", () => {
    const stale = { ...version, reference: { ...memory, revision: 1 } } as MemoryVersion;
    expect(() => deliverInputMemories(workspace, [stale])).toThrow(/revision/);
    const other = { ...version, reference: { ...memory, memory_id: "0199a000-0000-7000-8000-0000000000ff" } } as MemoryVersion;
    expect(() => deliverInputMemories(workspace, [other])).toThrow(/not an input/);
  });
});

describe("strict work result output schema", () => {
  const schema = workResultOutputSchema();
  const ajv = new Ajv2020({ strict: true, allErrors: true });
  const validate = ajv.compile(schema);
  function walk(node: unknown, path: string) {
    if (!node || typeof node !== "object") return;
    const object = node as Record<string, unknown>;
    if (object.type === "object" || (Array.isArray(object.type) && object.type.includes("object"))) {
      // OpenAI strict mode: every property is required and no extras are allowed.
      expect(object.additionalProperties, path).toBe(false);
      expect(Object.keys(object.properties as object).sort(), path).toEqual([...(object.required as string[])].sort());
    }
    expect(object.format, path).toBeUndefined();
    for (const [key, child] of Object.entries(object)) if (typeof child === "object") walk(child, `${path}/${key}`);
  }
  it("is strict-mode compliant at every level", () => {
    walk(schema, "#");
  });
  it("accepts the template and a completed result", () => {
    const template = workResultTemplate({ ...brief, inputs: { memories: [memory], sources: [], artifacts: [] } });
    expect(validate(template), JSON.stringify(validate.errors)).toBe(true);
    expect(validate(result), JSON.stringify(validate.errors)).toBe(true);
  });
  it("rejects the shapes the model produced without constrained sampling", () => {
    const bareReference = structuredClone(result) as unknown as { findings: { supporting_memories: unknown }[] };
    bareReference.findings[0].supporting_memories = [memory.memory_id];
    expect(validate(bareReference)).toBe(false);
    const { unresolved_work: _dropped, ...missing } = result;
    expect(validate(missing)).toBe(false);
    const objectsInCoverage = { ...result, coverage: { examined: [{ memory_id: "x" }], unexamined: [] } };
    expect(validate(objectsInCoverage)).toBe(false);
  });
});
