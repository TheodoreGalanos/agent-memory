/* Generated from Rust contracts by npm run generate. */

export type EvidentialStatus = "observation" | "attributed_statement" | "inference" | "assumption" | "simulation";
export type Origin = "observed" | "activated" | "agent_generated";
export type EffectStatus = "confirmed" | "not_performed" | "unknown";
export type WireVersion = "1";
export type WorkStatus = "complete" | "partial" | "blocked";
export type UsageStatus = "known" | "partial" | "unknown";
export type Process = "formation" | "activation" | "consolidation" | "maintenance" | "investigation" | "evaluation";
export type JobState = "queued" | "leased" | "running" | "waiting" | "completed" | "partial" | "failed" | "cancelled";

export interface Assignment {
  epoch: number;
  expires_at: string;
  job: Job;
  owner_id: string;
}
export interface Job {
  attempt: number;
  cancel_requested: boolean;
  deadline: string;
  depth: number;
  id: string;
  operation_id: string;
  ready_at?: string | null;
  result?: WorkResult | null;
  root_id: string;
  session_id: string;
  spec: SubmitJob;
  state: JobState;
  wait_reason?: string | null;
}
export interface WorkResult {
  child_outputs: string[];
  coverage: Coverage;
  examined_scope: Scope;
  findings: Finding[];
  inputs: WorkInputs;
  known_effects: KnownEffect[];
  proposed_changes: string[];
  result_artifact?: string | null;
  schema_version: WireVersion;
  status: WorkStatus;
  unresolved_work: string[];
  usage: Usage;
}
export interface Coverage {
  examined: string[];
  unexamined: string[];
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
export interface SourceRef {
  revision: string;
  source_id: string;
}
export interface Finding {
  applicability: Scope;
  challenging_memories: MemoryRef[];
  evidential_status: EvidentialStatus;
  origin: Origin;
  sources: SourceRef[];
  statement: string;
  supporting_memories: MemoryRef[];
}
export interface MemoryRef {
  label: string;
  memory_id: string;
  revision: number;
}
export interface WorkInputs {
  artifacts: string[];
  memories: MemoryRef[];
  sources: SourceRef[];
}
export interface KnownEffect {
  description: string;
  effect_id: string;
  status: EffectStatus;
}
export interface Usage {
  cost?: Money | null;
  input_tokens?: number | null;
  output_tokens?: number | null;
  status: UsageStatus;
}
export interface Money {
  currency: string;
  minor_units: number;
}
export interface SubmitJob {
  brief: WorkBrief;
  max_attempts: number;
  parent_id?: string | null;
  retain_until: string;
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
