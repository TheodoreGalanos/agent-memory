/* Generated from Rust contracts by npm run generate. */

export type EvidentialStatus = "observation" | "attributed_statement" | "inference" | "assumption" | "simulation";
export type EvidenceExposure = "parent_read" | "delegated_finding" | "not_read";
export type WorkspaceKind =
  "goal" | "constraint" | "observation" | "hypothesis" | "plan" | "obligation" | "dependency" | "artifact" | "memory";
export type ScratchLifetime =
  | {
      kind: "task";
    }
  | {
      kind: "phase";
      phase: string;
    }
  | {
      expires_at: string;
      kind: "until";
    };
export type Origin = "observed" | "activated" | "agent_generated";
export type WorkingStatus = "active" | "needs_revalidation" | "completed";
export type ValidTime =
  | {
      kind: "unknown";
    }
  | {
      from?: string | null;
      kind: "interval";
      to?: string | null;
    };
export type WireVersion = "1";
export type SourceCoverage = "examined" | "unexamined" | "unavailable";

export interface WorkspaceState {
  change_cursor?: number | null;
  conflicts: ConflictGroup[];
  context_groups?: ContextBundle[];
  entries: WorkspaceEntry[];
  expires_at: string;
  phase: string;
  /**
   * Recovery retention is not permission to form collection memories.
   */
  recovery_until: string;
  schema_version: WireVersion;
  scope: Scope;
  selected_contributions: string[];
  sources: SourceInventoryItem[];
  task_id: string;
  temporary: boolean;
  workspace_id: string;
}
export interface ConflictGroup {
  id: string;
  members: string[];
  status: string;
  unresolved: boolean;
}
export interface ContextBundle {
  id: string;
  members: string[];
  reason: string;
}
export interface WorkspaceEntry {
  decision_relevant: boolean;
  evidential_status: EvidentialStatus;
  exposure: EvidenceExposure;
  generating_operation?: string | null;
  id: string;
  inputs: WorkInputs;
  kind: WorkspaceKind;
  lifetime: ScratchLifetime;
  origin: Origin;
  priority: number;
  scope: Scope;
  status: WorkingStatus;
  supporting_entries: string[];
  /**
   * A concise, complete statement; large detail stays in referenced artifacts.
   */
  text: string;
  valid_time: ValidTime;
}
export interface WorkInputs {
  artifacts: string[];
  memories: MemoryRef[];
  sources: SourceRef[];
}
export interface MemoryRef {
  label: string;
  memory_id: string;
  revision: number;
}
export interface SourceRef {
  revision: string;
  source_id: string;
}
/**
 * Absent dimensions are unrestricted. An assigned restriction cannot be removed
 * by leaving that dimension out of a request. Tenant is checked separately.
 */
export interface Scope {
  entity_ids: string[];
  project_id?: string | null;
  source_versions: SourceRef[];
  task_id?: string | null;
  user_id?: string | null;
}
export interface SourceInventoryItem {
  coverage: SourceCoverage;
  description: string;
  source: SourceRef;
}
