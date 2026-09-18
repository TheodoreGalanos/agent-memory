// ABOUTME: Task execution helpers for pool workers: deliver granted input memories into the
// ABOUTME: workspace, and constrain the model's reply to the WorkResult contract at the provider.
import type { WorkResult } from "../../../contracts/generated/work-result.js";
import type { WorkBrief } from "../../../contracts/generated/work-command.js";
import type { WorkspaceState } from "../../../contracts/generated/workspace-state.js";
import type { MemoryVersion } from "../../../contracts/generated/host-response.js";

/** Granted input memories enter the workspace as label-only entries. This delivers their current
 * content in the same entry shape activation uses, so the render manifest records what the model saw.
 * Activation retrieval of additional memories is a separate process and is not performed here. */
export function deliverInputMemories(workspace: WorkspaceState, memories: MemoryVersion[]): WorkspaceState {
  const next = structuredClone(workspace);
  for (const version of memories) {
    const { reference, record } = version;
    const entry = next.entries.find((e) => e.kind === "memory" && e.inputs.memories.some((m) => m.memory_id === reference.memory_id));
    if (!entry) throw new Error(`Memory ${reference.memory_id} is not an input of this workspace`);
    const granted = entry.inputs.memories.find((m) => m.memory_id === reference.memory_id)!;
    if (granted.revision !== reference.revision)
      throw new Error(`Memory ${reference.memory_id} is at revision ${reference.revision}; the brief granted revision ${granted.revision}`);
    entry.text = JSON.stringify({ label: record.label, content: record.content, qualification: record.qualification });
    entry.origin = record.origin;
    entry.evidential_status = record.evidential_status;
    entry.valid_time = record.valid_time;
    entry.exposure = "parent_read";
    entry.decision_relevant = true;
  }
  return next;
}

/** The model completes findings, coverage and unresolved work; scope and inputs stay fixed by the brief.
 * The Host rejects `complete` while anything is unexamined or unresolved, so the template starts partial. */
export function workResultTemplate(brief: WorkBrief): WorkResult {
  return {
    schema_version: "1",
    status: "partial",
    examined_scope: brief.scope,
    inputs: brief.inputs,
    findings: [],
    coverage: { examined: [], unexamined: [] },
    unresolved_work: [],
    proposed_changes: [],
    child_outputs: [],
    known_effects: [],
    usage: { status: "unknown", input_tokens: null, output_tokens: null, cost: null },
  };
}


/** Instructions for a task model that must answer with the WorkResult contract. */
export function taskSystemPrompt(brief: WorkBrief): string {
  const template = workResultTemplate(brief);
  const exampleFinding = {
    statement: "One sentence stating what a supplied memory says or what you infer from it.",
    origin: "observed", evidential_status: "attributed_statement", applicability: brief.scope, sources: [],
    supporting_memories: brief.inputs.memories, challenging_memories: [],
  };
  return [
    "You are completing one bounded memory task. The workspace below is your only evidence; do not invent sources or values.",
    "Distinguish what the supplied memories state from what you infer. A value reported by a user is an attributed statement (origin observed, evidential_status attributed_statement); your own conclusions have origin agent_generated and evidential_status inference.",
    "Reply with exactly one JSON object and nothing else: the WorkResult below with findings, coverage.examined, coverage.unexamined and unresolved_work completed.",
    "status is \"partial\" whenever coverage.unexamined or unresolved_work is non-empty; use \"complete\" only when both are empty. Copy every identifier character for character.",
    "Copy examined_scope, inputs, usage, proposed_changes, child_outputs and known_effects exactly as given; never add, remove or edit entries in them.",
    `Memory references are exactly these objects and nothing else: ${JSON.stringify(brief.inputs.memories)}. Workspace entry ids such as "memory-0" are not memory references and must not appear in the result.`,
    "coverage.examined and coverage.unexamined are arrays of short plain strings describing what you did and did not examine. unresolved_work is an array of short plain strings.",
    `Every finding has exactly this shape: ${JSON.stringify(exampleFinding)}`,
    JSON.stringify(template),
  ].join("\n");
}

/** Azure Responses payload with the WorkResult contract as a strict output format. */
export function constrainToWorkResult(payload: unknown): unknown {
  const raw = payload as Record<string, unknown>;
  const text = (raw.text ?? {}) as Record<string, unknown>;
  return { ...raw, text: { ...text, format: { type: "json_schema", name: "work_result", strict: true, schema: workResultOutputSchema() } } };
}

const strictObject = (properties: Record<string, unknown>) => ({
  type: "object",
  properties,
  required: Object.keys(properties),
  additionalProperties: false,
});
const array = (items: unknown) => ({ type: "array", items });
const nullable = (type: string) => ({ type: [type, "null"] });

/** The WorkResult contract as an OpenAI strict output schema: every key present, no extras,
 * enum values fixed. Mirrors contracts/generated/work-result.schema.json without formats. */
export function workResultOutputSchema() {
  const scope = strictObject({
    user_id: nullable("string"),
    project_id: nullable("string"),
    task_id: nullable("string"),
    entity_ids: array({ type: "string" }),
    source_versions: array(strictObject({ source_id: { type: "string" }, revision: { type: "string" } })),
  });
  const memoryRef = strictObject({ memory_id: { type: "string" }, revision: { type: "integer" }, label: { type: "string" } });
  const sourceRef = strictObject({ source_id: { type: "string" }, revision: { type: "string" } });
  const finding = strictObject({
    statement: { type: "string" },
    origin: { type: "string", enum: ["observed", "activated", "agent_generated"] },
    evidential_status: { type: "string", enum: ["observation", "attributed_statement", "inference", "assumption", "simulation"] },
    applicability: scope,
    sources: array(sourceRef),
    supporting_memories: array(memoryRef),
    challenging_memories: array(memoryRef),
  });
  return strictObject({
    schema_version: { type: "string", enum: ["1"] },
    status: { type: "string", enum: ["complete", "partial", "blocked"] },
    examined_scope: scope,
    inputs: strictObject({ sources: array(sourceRef), memories: array(memoryRef), artifacts: array({ type: "string" }) }),
    findings: array(finding),
    coverage: strictObject({ examined: array({ type: "string" }), unexamined: array({ type: "string" }) }),
    unresolved_work: array({ type: "string" }),
    proposed_changes: array({ type: "string" }),
    child_outputs: array({ type: "string" }),
    known_effects: array(strictObject({
      effect_id: { type: "string" },
      status: { type: "string", enum: ["confirmed", "not_performed", "unknown"] },
      description: { type: "string" },
    })),
    usage: strictObject({
      status: { type: "string", enum: ["known", "partial", "unknown"] },
      input_tokens: nullable("integer"),
      output_tokens: nullable("integer"),
      cost: { anyOf: [strictObject({ currency: { type: "string" }, minor_units: { type: "integer" } }), { type: "null" }] },
    }),
  });
}
