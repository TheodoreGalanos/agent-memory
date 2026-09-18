/* Generated from Rust contracts by npm run generate. */

export type WireVersion = "1";
export type Process = "formation" | "activation" | "consolidation" | "maintenance" | "investigation" | "evaluation";

export interface Command {
  actor_id: string;
  budget_id: string;
  command_version: WireVersion;
  deadline: string;
  expected_revisions: MemoryRef[];
  invocation_id?: string | null;
  job_id?: string | null;
  lane_id?: string | null;
  lease_epoch?: number | null;
  operation_id?: string | null;
  payload: WorkBrief;
  request_id: string;
  scope: Scope;
  session_id?: string | null;
  tenant_id: string;
}
export interface MemoryRef {
  label: string;
  memory_id: string;
  revision: number;
}
export interface WorkBrief {
  capabilities: Capabilities;
  definitions: string[];
  disclosure_policy: string;
  evidence_cutoff: string;
  frame_ref?: string | null;
  freshness_requirement: string;
  inputs: WorkInputs;
  known_conflicts: string[];
  limits: WorkLimits;
  output_criteria: string[];
  policy: ConfigRef;
  process: Process;
  profile: ConfigRef;
  purpose: string;
  retention_policy: string;
  schema_version: WireVersion;
  scope: Scope;
  task_id: string;
}
export interface Capabilities {
  queries: string[];
  sources: SourceRef[];
  tools: string[];
}
export interface SourceRef {
  revision: string;
  source_id: string;
}
export interface WorkInputs {
  artifacts: string[];
  memories: MemoryRef[];
  sources: SourceRef[];
}
export interface WorkLimits {
  max_child_concurrency: number;
  max_child_depth: number;
  max_output_bytes: number;
  max_provider_attempts: number;
  max_tokens: number;
  root_budget_id: string;
}
export interface ConfigRef {
  id: string;
  label: string;
  revision: number;
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
