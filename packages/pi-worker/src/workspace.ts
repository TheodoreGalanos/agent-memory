import { randomUUID } from "node:crypto";
import type { WorkBrief } from "../../../contracts/generated/work-command.js";
import type {
  WorkspaceState,
  WorkspaceEntry,
  Scope,
  WorkInputs,
  ValidTime,
} from "../../../contracts/generated/workspace-state.js";

export type { WorkspaceState, WorkspaceEntry };
export const emptyInputs = (): WorkInputs => ({
  sources: [],
  memories: [],
  artifacts: [],
});

export function permits(grant: Scope, requested: Scope): boolean {
  return (
    (["user_id", "project_id", "task_id"] as const).every(
      (key) => !grant[key] || grant[key] === requested[key],
    ) &&
    (!grant.entity_ids.length ||
      (requested.entity_ids.length > 0 &&
        requested.entity_ids.every((id) => grant.entity_ids.includes(id)))) &&
    (!grant.source_versions.length ||
      (requested.source_versions.length > 0 &&
        requested.source_versions.every((ref) =>
          grant.source_versions.some(
            (g) => g.source_id === ref.source_id && g.revision === ref.revision,
          ),
        )))
  );
}

export interface WorkspaceAccess {
  /** Changes whenever the host's disclosure decision changes. No credentials. */
  revision: string;
  allows(scope: Scope, inputs: WorkInputs): boolean;
}

export function briefAccess(brief: WorkBrief): WorkspaceAccess {
  const granted = structuredClone(brief);
  return {
    revision: JSON.stringify([
      granted.scope,
      granted.capabilities.sources,
      granted.inputs,
    ]),
    allows: (scope, inputs) =>
      permits(granted.scope, scope) &&
      inputs.sources.every((ref) =>
        granted.capabilities.sources.some(
          (g) => g.source_id === ref.source_id && g.revision === ref.revision,
        ),
      ) &&
      inputs.memories.every((ref) =>
        granted.inputs.memories.some(
          (g) => g.memory_id === ref.memory_id && g.revision === ref.revision,
        ),
      ) &&
      inputs.artifacts.every((id) => granted.inputs.artifacts.includes(id)),
  };
}

export function workspaceFromBrief(
  brief: WorkBrief,
  expiresAt: string,
  recoveryUntil: string,
): WorkspaceState {
  const entry = (
    id: string,
    kind: WorkspaceEntry["kind"],
    text: string,
  ): WorkspaceEntry => ({
    id,
    kind,
    text,
    origin: "observed",
    evidential_status: "attributed_statement",
    scope: structuredClone(brief.scope),
    valid_time: { kind: "unknown" },
    inputs: emptyInputs(),
    generating_operation: null,
    supporting_entries: [],
    exposure: "parent_read",
    lifetime: { kind: "task" },
    status: "active",
    decision_relevant: true,
    priority: 0,
  });
  return {
    schema_version: "1",
    workspace_id: randomUUID(),
    task_id: brief.task_id,
    scope: structuredClone(brief.scope),
    phase: "work",
    expires_at: expiresAt,
    recovery_until: recoveryUntil,
    temporary: false,
    selected_contributions: [],
    entries: [
      entry("goal", "goal", brief.purpose),
      entry(
        "governing-frame",
        "constraint",
        JSON.stringify({
          definitions: brief.definitions,
          scope: brief.scope,
          evidence_cutoff: brief.evidence_cutoff,
          freshness_requirement: brief.freshness_requirement,
          output_criteria: brief.output_criteria,
          known_conflicts: brief.known_conflicts,
          policy: brief.policy,
          profile: brief.profile,
          retention_policy: brief.retention_policy,
          disclosure_policy: brief.disclosure_policy,
          capabilities: {
            tools: brief.capabilities.tools,
            queries: brief.capabilities.queries,
          },
          limits: brief.limits,
        }),
      ),
      ...brief.inputs.memories.map((memory, i) => ({
        ...entry(`memory-${i}`, "memory", memory.label ?? memory.memory_id),
        origin: "activated" as const,
        inputs: { ...emptyInputs(), memories: [memory] },
        exposure: "not_read" as const,
        decision_relevant: false,
      })),
      ...brief.inputs.artifacts.map((id, i) => ({
        ...entry(`artifact-${i}`, "artifact", `Available artifact ${id}`),
        inputs: { ...emptyInputs(), artifacts: [id] },
        exposure: "not_read" as const,
        decision_relevant: false,
      })),
    ],
    sources: brief.inputs.sources.map((source) => ({
      source,
      coverage: "unexamined",
      description: "Assigned evidence",
    })),
    conflicts: [],
  };
}

export function alive(
  state: WorkspaceState,
  entry: WorkspaceEntry,
  now: number,
): boolean {
  return (
    Date.parse(state.expires_at) > now &&
    (entry.lifetime.kind !== "phase" || entry.lifetime.phase === state.phase) &&
    (entry.lifetime.kind !== "until" ||
      Date.parse(entry.lifetime.expires_at) > now)
  );
}

export function validateWorkspace(state: WorkspaceState): void {
  if (
    state.schema_version !== "1" ||
    !Number.isFinite(Date.parse(state.expires_at)) ||
    !Number.isFinite(Date.parse(state.recovery_until))
  )
    throw new Error("Invalid workspace lifetime or schema");
  const entries = new Map(state.entries.map((e) => [e.id, e]));
  if (entries.size !== state.entries.length)
    throw new Error("Duplicate workspace entry ID");
  if (!state.entries.some((e) => e.kind === "goal" && e.status === "active"))
    throw new Error("Workspace needs an active goal");
  for (const entry of state.entries) {
    if (!entry.id || !entry.text.trim() || !permits(state.scope, entry.scope))
      throw new Error("Invalid workspace entry or scope");
    if (entry.supporting_entries.some((id) => !entries.has(id)))
      throw new Error("Missing supporting entry");
    if (entry.exposure === "delegated_finding" && !entry.generating_operation)
      throw new Error("Delegated finding needs its generating operation");
    if (
      entry.valid_time.kind === "interval" &&
      entry.valid_time.from &&
      entry.valid_time.to &&
      Date.parse(entry.valid_time.from) >= Date.parse(entry.valid_time.to)
    )
      throw new Error("Invalid valid-time interval");
  }
  const visiting = new Set<string>();
  const visited = new Set<string>();
  function visit(id: string) {
    if (visiting.has(id)) throw new Error("Workspace support cycle");
    if (visited.has(id)) return;
    visiting.add(id);
    for (const support of entries.get(id)!.supporting_entries) visit(support);
    visiting.delete(id);
    visited.add(id);
  }
  entries.forEach((e) => visit(e.id));
  for (const group of [...state.conflicts, ...(state.context_groups ?? [])]) {
    if (
      new Set(group.members).size < 2 ||
      group.members.some((id) => !entries.has(id))
    )
      throw new Error("Context group needs all member entries");
  }
  if (new Set(state.conflicts.map((g) => g.id)).size !== state.conflicts.length)
    throw new Error("Duplicate conflict group ID");
  if (
    new Set((state.context_groups ?? []).map((g) => g.id)).size !==
    (state.context_groups ?? []).length
  )
    throw new Error("Duplicate context group ID");
  if (state.selected_contributions.some((id) => !entries.has(id)))
    throw new Error("Unknown selected contribution");
}

/** This is an eligibility decision for WP09, not a write to the collection. */
export function formationInputs(
  state: WorkspaceState,
  now = Date.now(),
): WorkspaceEntry[] {
  return state.entries.filter(
    (e) =>
      alive(state, e, now) &&
      e.status !== "needs_revalidation" &&
      (!state.temporary || state.selected_contributions.includes(e.id)),
  );
}

export interface WorkspaceChange {
  previous: WorkInputs;
  kind: "wording" | "substantive" | "access";
  scope: Scope;
  validTime: ValidTime;
}
function overlaps(a: ValidTime, b: ValidTime): boolean {
  if (a.kind === "unknown" || b.kind === "unknown") return true;
  return (
    (a.from ? Date.parse(a.from) : -Infinity) <
      (b.to ? Date.parse(b.to) : Infinity) &&
    (b.from ? Date.parse(b.from) : -Infinity) <
      (a.to ? Date.parse(a.to) : Infinity)
  );
}
function references(a: WorkInputs, b: WorkInputs): boolean {
  return (
    a.sources.some((x) =>
      b.sources.some(
        (y) => x.source_id === y.source_id && x.revision === y.revision,
      ),
    ) ||
    a.memories.some((x) =>
      b.memories.some(
        (y) => x.memory_id === y.memory_id && x.revision === y.revision,
      ),
    ) ||
    a.artifacts.some((x) => b.artifacts.includes(x))
  );
}
function scopeOverlaps(a: Scope, b: Scope): boolean {
  return (
    (["user_id", "project_id", "task_id"] as const).every(
      (key) => !a[key] || !b[key] || a[key] === b[key],
    ) &&
    (!a.entity_ids.length ||
      !b.entity_ids.length ||
      a.entity_ids.some((id) => b.entity_ids.includes(id)))
  );
}

/** Recheck unfinished conclusions only; their cited basis is never silently rewritten. */
export function reconcileWorkspace(
  state: WorkspaceState,
  changes: WorkspaceChange[],
  now = Date.now(),
): WorkspaceState {
  const next = structuredClone(state);
  const affected = new Set<string>();
  for (const e of next.entries) {
    if (
      e.status !== "completed" &&
      alive(next, e, now) &&
      changes.some(
        (change) =>
          change.kind !== "wording" &&
          references(e.inputs, change.previous) &&
          scopeOverlaps(e.scope, change.scope) &&
          (change.kind === "access" ||
            overlaps(e.valid_time, change.validTime)),
      )
    )
      affected.add(e.id);
  }
  let grew = true;
  while (grew) {
    grew = false;
    for (const e of next.entries)
      if (
        e.status !== "completed" &&
        alive(next, e, now) &&
        !affected.has(e.id) &&
        e.supporting_entries.some((id) => affected.has(id))
      ) {
        affected.add(e.id);
        grew = true;
      }
  }
  for (const e of next.entries)
    if (affected.has(e.id)) e.status = "needs_revalidation";
  return next;
}
