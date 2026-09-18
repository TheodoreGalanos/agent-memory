/* Generated from Rust contracts by npm run generate. */

export type HostRequest =
  | {
      action: "user";
      request: UserRequest;
    }
  | {
      action: "record_context";
      fence: Fence;
      manifest: RenderManifest;
    }
  | {
      action: "check_provider";
      fence: Fence;
      provider: string;
    }
  | {
      action: "list_deletions";
      after_id?: string | null;
      limit: number;
    }
  | {
      action: "reconcile_deleted_effect";
      deletion: string;
      effect_id: string;
      evidence_artifact: string;
      resolution: EffectResolution;
    }
  | {
      action: "continue_deleted_work";
      command: Command;
      deletion: string;
      old_job: string;
    }
  | {
      action: "begin_deletion";
      request: DeletionRequest;
    }
  | {
      action: "inspect_deletion";
      id: string;
    }
  | {
      action: "purge_deletion";
      id: string;
    }
  | {
      action: "acknowledge_purge";
      boundary: RetentionBoundary;
      container: string;
      id: string;
    }
  | {
      action: "restore_deletion_registry";
      reports: DeletionReport[];
    }
  | {
      action: "review_maintenance";
      fence: Fence;
      id: string;
      request: MaintenanceRequest;
    }
  | {
      action: "commit_maintenance";
      fence: Fence;
      request: MaintenanceCommit;
    }
  | {
      action: "memory_changes";
      after: number;
      fence: Fence;
      limit: number;
    }
  | {
      action: "current_memories";
      fence: Fence;
      references: MemoryRef[];
    }
  | {
      action: "intention_check";
      fence: Fence;
      id: string;
      kind: IntentionCheckKind;
      occurrence_id: string;
    }
  | {
      action: "apply_intention_check";
      check_id: string;
      decisions: {
        [k: string]: string;
      };
      fence: Fence;
    }
  | {
      action: "inspect_intentions";
      definition_id: string;
    }
  | {
      action: "cancel_intention";
      occurrence_id: string;
      reason: string;
    }
  | {
      action: "confirm_intention";
      occurrence_id: string;
    }
  | {
      action: "sweep_intentions";
      limit: number;
    }
  | {
      action: "consolidation_window";
      fence: Fence;
      id: string;
      selection: CohortSelection;
    }
  | {
      action: "review_consolidation";
      fence: Fence;
      id: string;
      proposal: ConsolidationProposal;
    }
  | {
      action: "commit_consolidation";
      fence: Fence;
      request: ConsolidationCommit;
    }
  | {
      action: "start_qualification";
      fence: Fence;
      request_id: string;
      review_id: string;
      suite_id: string;
    }
  | {
      action: "adopt_procedure";
      evaluation_job: string;
      fence: Fence;
    }
  | {
      action: "embedding_inputs";
      after?: SearchCursor | null;
      limit: number;
    }
  | {
      action: "activate";
      cursor?: ActivationCursor | null;
      fence: Fence;
      id: string;
      query: ActivationQuery;
    }
  | {
      action: "select_activation";
      fence: Fence;
      selection: ActivationSelection;
    }
  | {
      action: "save_embedding";
      embedding: Embedding;
      reference: MemoryRef;
    }
  | {
      action: "formation_window";
      fence: Fence;
      id: string;
      limit: number;
      operation: string;
      source: SourceRef;
    }
  | {
      action: "commit_formation";
      fence: Fence;
      request: FormationCommit;
    }
  | {
      action: "admit_judgement";
      fence: Fence;
      packet: JudgementPacket;
    }
  | {
      action: "check_judgement";
      fence: Fence;
      packet_id: string;
      provider: string;
    }
  | {
      action: "record_assessment";
      assessment: SemanticAssessment;
      fence: Fence;
    }
  | {
      action: "reuse_assessment";
      assessment_id: string;
      fence: Fence;
      model_release: string;
      packet_id: string;
      provider: string;
    }
  | {
      action: "record_judgement_decision";
      decision: JudgementDecision;
      fence: Fence;
    }
  | {
      action: "register_task_check";
      check: TaskLocalCheck;
      fence: Fence;
    }
  | {
      action: "inspect_task_check";
      fence: Fence;
      id: string;
    }
  | {
      action: "retire_task_check";
      fence: Fence;
      id: string;
    }
  | {
      action: "create_budget";
      budget: Budget;
    }
  | {
      action: "submit";
      command: Command;
    }
  | {
      action: "spawn_child";
      brief: WorkBrief;
      deadline: string;
      fence: Fence;
      request_id: string;
      reuse_job_id?: string | null;
    }
  | {
      action: "child_jobs";
      fence: Fence;
      ids: string[];
    }
  | {
      action: "wait_children";
      fence: Fence;
      ids: string[];
      ready_at: string;
    }
  | {
      action: "read_input";
      artifact_id: string;
      fence: Fence;
      limit: number;
      offset: number;
    }
  | {
      action: "publish_artifact";
      dependencies: string[];
      fence: Fence;
      label: string;
      request_id: string;
      text: string;
    }
  | {
      action: "inspect_job";
      job_id: string;
    }
  | {
      action: "backup";
      directory: string;
    }
  | {
      action: "claim_next";
      lease_seconds: number;
      processes: Process[];
    }
  | {
      action: "issue_worker_credential";
      job_id: string;
    }
  | {
      action: "ingest_source";
      content: unknown;
      source: SourceVersion;
    }
  | {
      action: "claim";
      job_id: string;
      lease_seconds: number;
    }
  | {
      action: "start";
      fence: Fence;
    }
  | {
      action: "inspect_assignment";
      fence: Fence;
    }
  | {
      action: "renew";
      fence: Fence;
      lease_seconds: number;
    }
  | {
      action: "wait";
      fence: Fence;
      ready_at: string;
      reason: string;
    }
  | {
      action: "cancel";
      job_id: string;
    }
  | {
      action: "acknowledge_cancellation";
      fence: Fence;
    }
  | {
      action: "complete";
      fence: Fence;
      request_id: string;
      result: WorkResult;
    }
  | {
      action: "commit_memories";
      command: Command2;
    }
  | {
      action: "reserve";
      fence: Fence;
      final_result: boolean;
      maximum: Resources;
      provider_attempt: string;
    }
  | {
      action: "settle_usage";
      fence: Fence;
      observed?: Resources | null;
      reservation_id: string;
    }
  | {
      action: "budget_usage";
      budget_id: string;
    }
  | {
      action: "prepare_effect";
      fence: Fence;
      request: EffectRequest;
    }
  | {
      action: "begin_effect";
      effect_id: string;
      fence: Fence;
    }
  | {
      action: "report_effect";
      effect_id: string;
      fence: Fence;
      receipt: unknown;
      state: EffectState;
    }
  | {
      action: "inspect_effect";
      effect_id: string;
    }
  | {
      action: "reconcile_effect";
      effect_id: string;
      evidence: unknown;
      resolution: EffectResolution;
    }
  | {
      action: "events";
      after: number;
      limit: number;
    }
  | {
      action: "consume_event";
      command: Command;
      consumer: string;
      event_id: string;
    }
  | {
      action: "allocate_artifact";
      id: string;
      spec: ArtifactSpec;
    }
  | {
      action: "recover_artifact_upload";
      id: string;
      revision: number;
    }
  | {
      action: "recover";
    }
  | {
      action: "prune_history";
    };
export type UserRequest =
  | {
      action: "identity";
    }
  | {
      action: "browse";
      query: MemoryQuery;
    }
  | {
      action: "history";
      after_revision: number;
      limit: number;
      memory_id: string;
    }
  | {
      action: "inspect_memory";
      reference: MemoryRef;
    }
  | {
      action: "inspect_task";
      after_manifest?: string | null;
      job_id: string;
    }
  | {
      action: "inspect_exploration";
      id: string;
    }
  | {
      action: "decisions";
      job_id: string;
    }
  | {
      action: "changes";
      after: number;
      limit: number;
    }
  | {
      action: "notifications";
      after: number;
      limit: number;
    }
  | {
      action: "policy";
      reference: ConfigRef;
    }
  | {
      action: "controls";
    }
  | {
      action: "mutate";
      mutation: UserMutation;
      request_id: string;
    };
export type UserMutation =
  | {
      action: "contribute";
      record: RecordDraft;
    }
  | {
      action: "correct";
      expected: MemoryRef;
      record: RecordDraft;
    }
  | {
      action: "open_exploration";
      expires_at: string;
      purpose: string;
      scope: Scope;
    }
  | {
      action: "add_exploration";
      expected_revision: number;
      id: string;
      record: RecordDraft;
    }
  | {
      action: "promote_exploration";
      expected_revision: number;
      id: string;
      index: number;
    }
  | {
      action: "request_decision";
      deadline: string;
      fence?: Fence | null;
      job_id: string;
      missing: string;
      owner_id: string;
      question: string;
    }
  | {
      action: "answer_decision";
      answer: DecisionAnswer;
      expected_revision: number;
      id: string;
      reason: string;
    }
  | {
      action: "notification_preference";
      mode: NotificationMode;
    }
  | {
      action: "acknowledge_notification";
      event_id: string;
    }
  | {
      action: "set_control";
      enabled: boolean;
      expected_revision: number;
      kind: ControlKind;
      target: string;
    }
  | {
      action: "create_policy";
      effective: ValidTime;
      label: string;
      policy: MemoryPolicy;
      scope: Scope;
    }
  | {
      action: "revise_policy";
      effective: ValidTime;
      expected: ConfigRef;
      policy: MemoryPolicy;
    };
export type Availability = "routine" | "historical" | "retired" | "quarantined";
export type MemoryContent =
  | {
      actions: string[];
      corrections: string[];
      family: "episode";
      initial_conditions: string[];
      objective: string;
      observations: string[];
      outcome: string;
      uncertainty: string[];
      verification: string[];
    }
  | {
      examined_coverage: string[];
      family: "knowledge";
      predicate?: string | null;
      statement: string;
      subject?: string | null;
      uncertainty: string[];
    }
  | {
      applicability: string[];
      capabilities: string[];
      contract?: ProcedureContract | null;
      counterexamples: MemoryRef[];
      exclusions: string[];
      family: "procedure";
      method: ProcedureForm;
      purpose: string;
    }
  | {
      completion: string[];
      expires_at?: string | null;
      family: "intention";
      notification_policy: string;
      owner_id: string;
      plan?: IntentionPlan | null;
      purpose: string;
      readiness: string[];
      recurrence?: string | null;
      trigger: string;
    };
export type QuestionForm =
  | {
      criteria: {
        [k: string]: string;
      };
      type: "choice";
    }
  | {
      criteria: {
        [k: string]: string;
      };
      type: "noul";
    }
  | {
      criteria: string[];
      type: "score";
    };
export type ReplayClass = "observation" | "remote_idempotent" | "reconcile";
export type ProcedureForm =
  | {
      evidence_criteria: string[];
      form: "advisory";
      steps: string[];
    }
  | {
      artifact_id: string;
      entrypoint: string;
      form: "executable";
      inputs: string[];
      outputs: string[];
    };
export type Process = "formation" | "activation" | "consolidation" | "maintenance" | "investigation" | "evaluation";
export type WireVersion = "1";
export type IntentionTrigger =
  | {
      at: string;
      kind: "time";
    }
  | {
      event_kind: string;
      kind: "event";
      resource_id: string;
    }
  | {
      after_revision: string;
      kind: "source_revision";
      source_id: string;
    }
  | {
      job_id: string;
      kind: "result";
    }
  | {
      definition: JudgementDefinition;
      kind: "semantic";
      matched_choice: string;
    };
export type PolicyAction =
  "retain" | "qualify" | "retrieve_further" | "investigate" | "revise" | "defer" | "retire" | "request_input";
export type EvidentialStatus = "observation" | "attributed_statement" | "inference" | "assumption" | "simulation";
export type Origin = "observed" | "activated" | "agent_generated";
export type Qualification =
  | {
      status: "candidate";
    }
  | {
      conditions: string[];
      evaluation_artifact?: string | null;
      evaluator_profile?: ConfigRef | null;
      evidence: MemoryRef[];
      status: "evaluated";
    }
  | {
      reason: string;
      status: "withdrawn";
    };
export type ValidTime =
  | {
      kind: "unknown";
    }
  | {
      from?: string | null;
      kind: "interval";
      to?: string | null;
    };
export type DecisionAnswer = "approve" | "decline";
export type NotificationMode = "material" | "blockers" | "completion" | "muted";
export type ControlKind = "provider" | "family" | "profile";
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
export type WorkingStatus = "active" | "needs_revalidation" | "completed";
export type SourceCoverage = "examined" | "unexamined" | "unavailable";
export type EffectResolution = "succeeded" | "failed" | "not_performed";
export type RetentionBoundary = "session" | "upload" | "sandbox" | "provider" | "backup" | "database_storage";
export type EffectState = "prepared" | "in_progress" | "succeeded" | "failed" | "outcome_unknown";
export type IntentionCheckKind =
  | {
      kind: "readiness";
    }
  | {
      event_id: string;
      kind: "trigger";
    }
  | {
      kind: "completion";
    };
export type JudgementAnswer =
  | {
      choice: string;
      confidence?: number | null;
      probabilities?: {
        [k: string]: number;
      } | null;
      type: "choice";
    }
  | {
      noul: number;
      type: "noul";
    }
  | {
      confidence?: number | null;
      legend: {
        [k: string]: string;
      };
      probabilities?: {
        [k: string]: number;
      } | null;
      score: number;
      type: "score";
    };
export type AssessmentStatus = "answered" | "invalid_response" | "unavailable";
export type SourceKind = "document" | "table" | "model" | "tool_events";
export type EffectStatus = "confirmed" | "not_performed" | "unknown";
export type WorkStatus = "complete" | "partial" | "blocked";
export type UsageStatus = "known" | "partial" | "unknown";
export type MemoryChange =
  | RecordDraft1
  | {
      action: "revise";
      expected: MemoryRef;
      record: RecordDraft;
    };

export interface MemoryQuery {
  after_id?: string | null;
  entity_id?: string | null;
  include_inactive?: boolean;
  limit?: number;
  recorded_as_of?: number | null;
  valid_at?: string | null;
}
export interface MemoryRef {
  label: string;
  memory_id: string;
  revision: number;
}
export interface ConfigRef {
  id: string;
  label: string;
  revision: number;
}
export interface RecordDraft {
  availability: Availability;
  content: MemoryContent;
  decision: PolicyDecision;
  derived_from: MemoryRef[];
  evidential_status: EvidentialStatus;
  label: string;
  origin: Origin;
  qualification: Qualification;
  scope: Scope;
  source_locators: string[];
  valid_time: ValidTime;
}
export interface ProcedureContract {
  checks: string[];
  decision_points: string[];
  effects: string[];
  parameters: {
    [k: string]: string;
  };
  recognition: RecognitionCriterion[];
  replay_class: ReplayClass;
  stopping_criteria: string[];
}
export interface RecognitionCriterion {
  definition: JudgementDefinition;
  model: string;
  model_release: string;
  policy: ConfigRef;
}
export interface JudgementDefinition {
  applicability: string;
  evaluation_refs: string[];
  fallback: string;
  id: string;
  input_requirements: string[];
  owner: string;
  permitted_uses: string[];
  questions: JudgementQuestion[];
  revision: number;
}
export interface JudgementQuestion {
  form: QuestionForm;
  instructions: string;
  key: string;
}
export interface IntentionPlan {
  completion: CompletionRule;
  execution: WorkBrief;
  ready_artifacts: string[];
  ready_memories: MemoryRef[];
  /**
   * Fixed intervals only; missed windows expire without a backlog of executions.
   */
  recurrence_seconds?: number | null;
  trigger: IntentionTrigger;
}
export interface CompletionRule {
  confirmation_owner?: string | null;
  /**
   * When set, a host-recorded job completion before expiry can be delivered within this grace.
   */
  delivery_grace_seconds?: number | null;
  equals: unknown;
  /**
   * Invocation IDs within the execution job. The Host supplies the operation prefix.
   */
  required_effects: string[];
  result_pointer: string;
  semantic_conditions: string[];
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
export interface PolicyDecision {
  action: PolicyAction;
  constraints: string[];
  expires_at?: string | null;
  policy: ConfigRef;
  reason: string;
  required_evidence: string[];
}
export interface Fence {
  epoch: number;
  job_id: string;
  owner_id: string;
}
export interface MemoryPolicy {
  allowed_uses: string[];
  applicability_rules: string[];
  budget_class: string;
  consolidation?: ConsolidationPolicy | null;
  evidence_requirements: string[];
  judgement_dispositions: string[];
  notification_policy: string;
  qualification_requirements: string[];
  retention_purpose: string;
  scheduling_priority: number;
  semantic_triggers?: JudgementDefinition[];
  source_rules: string[];
}
export interface ConsolidationPolicy {
  evaluator_profile: ConfigRef;
  maximum_cost_microunits: number;
  maximum_regression: number;
  min_held_out_groups: number;
  min_independent_sources: number;
  minimum_success_rate: number;
}
export interface RenderManifest {
  access_revision: string;
  conflicts: ConflictGroup[];
  decision_id: string;
  deferred: DeferredContext[];
  estimated_input_tokens: number;
  estimator: string;
  final_payload_bytes?: number | null;
  lane: string;
  /**
   * The projected transcript and tool schemas, never provider credentials.
   */
  messages: unknown[];
  model: string;
  next_step?: string | null;
  operation_id: string;
  profile: ConfigRef;
  provider: string;
  rebuild_reasons: string[];
  reserved_output_tokens: number;
  reserved_result_tokens: number;
  schema_version: WireVersion;
  selected: WorkspaceEntry[];
  session_id: string;
  sources: SourceInventoryItem[];
  status: string;
  strategy: string;
  tool_schemas: unknown[];
  usage: RenderUsage;
  workspace_id: string;
}
export interface ConflictGroup {
  id: string;
  members: string[];
  status: string;
  unresolved: boolean;
}
export interface DeferredContext {
  entry_ids: string[];
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
export interface SourceInventoryItem {
  coverage: SourceCoverage;
  description: string;
  source: SourceRef;
}
export interface RenderUsage {
  cache_read_tokens?: number | null;
  cache_write_tokens?: number | null;
  output_tokens?: number | null;
  uncached_input_tokens?: number | null;
}
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
  payload: SubmitJob;
  request_id: string;
  scope: Scope;
  session_id?: string | null;
  tenant_id: string;
}
export interface SubmitJob {
  brief: WorkBrief;
  max_attempts: number;
  parent_id?: string | null;
  retain_until: string;
}
export interface DeletionRequest {
  id: string;
  /**
   * Resource identities: memories, artifacts, jobs, sources, or source-version scope IDs.
   * A source identity is deleted across its retained revisions.
   */
  resources: string[];
}
export interface DeletionReport {
  access_denied: boolean;
  effects: DeletedEffect[];
  epoch: number;
  id: string;
  live_payloads_removed: boolean;
  obligations: PurgeObligation[];
  requested_resources: string[];
  resources: string[];
  revoked_ids: string[];
  scope: Scope;
  sessions: string[];
  support_review: string[];
  tenant_id: string;
}
export interface DeletedEffect {
  evidence_artifact?: string | null;
  id: string;
  state: EffectState;
}
export interface PurgeObligation {
  acknowledged: boolean;
  container: string;
  kind: RetentionBoundary;
}
export interface MaintenanceRequest {
  after?: RecordDraft | null;
  before: MemoryRef;
  /**
   * Bounded retrieval shortlist for indirect dependencies; J16 decides relevance.
   */
  candidates: MemoryRef[];
  reason: string;
  /**
   * Explicit removed source snapshots; the Host verifies their unavailable state.
   */
  removed_sources: SourceRef[];
}
export interface MaintenanceCommit {
  decisions: {
    [k: string]: string;
  };
  review_id: string;
}
export interface CohortSelection {
  mechanism: string;
  /**
   * Explicit shortlist obtained from scoped retrieval; linked exceptions are added by the host.
   */
  members: MemoryRef[];
  outcomes: string[];
  purpose: string;
  source_context: string;
}
export interface ConsolidationProposal {
  clauses: ConditionalClause[];
  concise_summary: string;
  invariant: string;
  label: string;
  /**
   * Omit for conditional knowledge. Procedure content includes its execution/investigation contract.
   */
  procedure?: MemoryContent | null;
  scope: Scope;
  untested: string[];
  variations: string[];
  window_id: string;
}
export interface ConditionalClause {
  conditions: string[];
  exceptions: MemoryRef[];
  support: MemoryRef[];
  text: string;
  uncertainty: string[];
}
export interface ConsolidationCommit {
  /**
   * J11 for the cohort and J12 for the complete proposal and each clause.
   */
  decisions: {
    [k: string]: string;
  };
  review_id: string;
}
export interface SearchCursor {
  sequence: number;
  version_id: string;
}
export interface ActivationCursor {
  after_id: string;
  cutoff: number;
  query: ActivationQuery;
}
export interface ActivationQuery {
  candidate_limit: number;
  context_bytes: number;
  entities: string[];
  exact: string[];
  existing: MemoryRef[];
  families: string[];
  fresh_after?: string | null;
  question: string;
  recorded_as_of?: number | null;
  scan_limit: number;
  scope: Scope;
  /**
   * Task evidence supplied to semantic applicability checks, not a collection write.
   */
  task_context: string;
  traversal_limit: number;
  valid_at?: string | null;
  vector?: Embedding | null;
}
export interface Embedding {
  identity: EmbeddingIdentity;
  values: number[];
}
export interface EmbeddingIdentity {
  dimensions: number;
  model: string;
  representation: string;
  revision: string;
}
export interface ActivationSelection {
  /**
   * candidate index / family / condition index -> WP08 decision ID.
   */
  decisions: {
    [k: string]: string;
  };
  window_id: string;
}
export interface FormationCommit {
  /**
   * One selected judgement decision per candidate event.
   */
  decisions: {
    [k: string]: string;
  };
  window_id: string;
}
export interface JudgementPacket {
  allowed_providers: string[];
  budget_id: string;
  deadline: string;
  disclosure_policy: string;
  evidence: JudgementEvidence[];
  evidence_cutoff: string;
  expires_at: string;
  frame: string;
  fresh_after: string;
  id: string;
  inputs: WorkInputs;
  job_id: string;
  local_check_id?: string | null;
  missing: string[];
  policy: ConfigRef;
  questions: PacketQuestion[];
  scope: Scope;
  subject: string;
}
export interface JudgementEvidence {
  artifact_id: string;
  content: unknown;
  coverage: string[];
  name: string;
  origin: Origin;
  pointer: string;
}
export interface PacketQuestion {
  allowed_providers: string[];
  definition: JudgementDefinition;
  /**
   * Named evidence fields containing results from earlier packets.
   */
  depends_on: string[];
  disclosure_scope: Scope;
}
export interface SemanticAssessment {
  answers: {
    [k: string]: JudgementAnswer;
  };
  assessed_at: string;
  expires_at: string;
  failure?: string | null;
  id: string;
  model_release?: string | null;
  packet_id: string;
  provider: string;
  raw_artifact_id: string;
  requested_model: string;
  reservation_id: string;
  returned_model?: string | null;
  status: AssessmentStatus;
  usage: JudgementUsage;
}
export interface JudgementUsage {
  cost_microunits?: number | null;
  input_tokens?: number | null;
  output_tokens?: number | null;
}
export interface JudgementDecision {
  assessment_ids: string[];
  decisions: {
    [k: string]: PolicyDecision;
  };
  id: string;
  inconsistencies: string[];
  mode: string;
  packet_id: string;
  selected_assessment?: string | null;
}
export interface TaskLocalCheck {
  allowed_providers: string[];
  completion_requirements: string[];
  definition: JudgementDefinition;
  disclosure_policy: string;
  expires_at: string;
  id: string;
  job_id: string;
  retired: boolean;
  scope: Scope;
  task_id: string;
}
export interface Budget {
  deadline: string;
  final_result_reserve: Resources;
  id: string;
  limit: Resources;
  max_child_concurrency: number;
  max_child_depth: number;
  pricing_revision: string;
  scope: Scope;
}
export interface Resources {
  cost_microunits: number;
  output_bytes: number;
  provider_calls: number;
  sandbox_cpu_ms: number;
  sandbox_time_ms: number;
  tokens: number;
}
export interface SourceVersion {
  acquired_at: string;
  acquisition_method: string;
  kind: SourceKind;
  label: string;
  owner: string;
  precedence?: string | null;
  reference: SourceRef;
  scope: Scope;
  snapshot_artifact?: string | null;
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
export interface Finding {
  applicability: Scope;
  challenging_memories: MemoryRef[];
  evidential_status: EvidentialStatus;
  origin: Origin;
  sources: SourceRef[];
  statement: string;
  supporting_memories: MemoryRef[];
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
export interface Command2 {
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
  payload: MemoryCommit;
  request_id: string;
  scope: Scope;
  session_id?: string | null;
  tenant_id: string;
}
export interface MemoryCommit {
  changes: MemoryChange[];
  fence: Fence;
}
export interface RecordDraft1 {
  availability: Availability;
  content: MemoryContent;
  decision: PolicyDecision;
  derived_from: MemoryRef[];
  evidential_status: EvidentialStatus;
  label: string;
  origin: Origin;
  qualification: Qualification;
  scope: Scope;
  source_locators: string[];
  valid_time: ValidTime;
}
export interface EffectRequest {
  arguments: unknown;
  invocation_id: string;
  kind: string;
  logical_operation_id: string;
  replay: ReplayClass;
}
export interface ArtifactSpec {
  dependencies: string[];
  expected_bytes: number;
  label: string;
  media_type: string;
  origin: Origin;
  retention_class: string;
  scope: Scope;
}
