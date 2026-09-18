/* Generated from Rust contracts by npm run generate. */

export type HostResponse =
  | {
      kind: "user";
      result: UserResponse;
    }
  | {
      kind: "deletions";
      reports: DeletionReport[];
    }
  | {
      kind: "deletion";
      report: DeletionReport;
    }
  | {
      kind: "maintenance_review";
      review: MaintenanceReview;
    }
  | {
      kind: "maintenance_result";
      result: MaintenanceResult;
    }
  | {
      kind: "changes";
      page: ChangePage;
    }
  | {
      check: IntentionCheck;
      kind: "intention_check";
    }
  | {
      kind: "intention";
      occurrence: IntentionOccurrence;
    }
  | {
      kind: "intentions";
      occurrences: IntentionOccurrence[];
    }
  | {
      kind: "intention_sweep";
      result: IntentionSweep;
    }
  | {
      kind: "consolidation_window";
      window: ConsolidationWindow;
    }
  | {
      kind: "consolidation_review";
      review: ConsolidationReview;
    }
  | {
      kind: "consolidation_result";
      result: ConsolidationResult;
    }
  | {
      kind: "adoption_result";
      result: AdoptionResult;
    }
  | {
      inputs: EmbeddingInput[];
      kind: "embedding_inputs";
    }
  | {
      kind: "activation_window";
      window: ActivationWindow;
    }
  | {
      kind: "context_package";
      package: ContextPackage;
    }
  | {
      kind: "formation_window";
      window: FormationWindow;
    }
  | {
      kind: "formation_result";
      result: FormationResult;
    }
  | {
      kind: "judgement_packet";
      packet: JudgementPacket;
    }
  | {
      assessment?: SemanticAssessment | null;
      kind: "assessment";
    }
  | {
      decision: JudgementDecision;
      kind: "judgement_decision";
    }
  | {
      check: TaskLocalCheck;
      kind: "task_check";
    }
  | {
      kind: "artifact_data";
      text: string;
    }
  | {
      jobs: Job[];
      kind: "children";
    }
  | {
      artifact: Artifact;
      kind: "artifact";
    }
  | {
      kind: "source";
      source: SourceVersion;
    }
  | {
      credential: IssuedWorkerCredential;
      kind: "worker_credential";
    }
  | {
      assignment: Assignment;
      credential: IssuedWorkerCredential;
      kind: "work";
    }
  | {
      kind: "backup";
      manifest: BackupManifest;
    }
  | {
      kind: "idle";
    }
  | {
      job: Job;
      kind: "job";
    }
  | {
      assignment: Assignment;
      kind: "assignment";
    }
  | {
      budget: Budget;
      kind: "budget";
    }
  | {
      kind: "budget_usage";
      usage: BudgetUsage;
    }
  | {
      kind: "reservation";
      reservation: Reservation;
    }
  | {
      effect: Effect;
      kind: "effect";
    }
  | {
      kind: "memories";
      memories: MemoryVersion[];
    }
  | {
      kind: "events";
      page: EventPage;
    }
  | {
      affected: number;
      kind: "done";
    };
export type UserResponse =
  | {
      actor_id: string;
      kind: "identity";
      scope: Scope;
      tenant_id: string;
    }
  | {
      kind: "records";
      next?: string | null;
      records: MemoryVersion[];
    }
  | {
      kind: "history";
      next_revision?: number | null;
      records: MemoryVersion[];
    }
  | {
      kind: "memory";
      memory: MemoryVersion;
      relations: RelationVersion[];
    }
  | {
      coverage: Coverage;
      decisions: DecisionRequest[];
      job: Job;
      kind: "task";
      manifests: RenderManifest[];
      next_manifest?: string | null;
    }
  | {
      exploration: Exploration;
      kind: "exploration";
    }
  | {
      decisions: DecisionRequest[];
      kind: "decisions";
    }
  | {
      kind: "changes";
      page: EventPage;
      revisions: MemoryChangeNotice[];
    }
  | {
      kind: "notifications";
      mode: NotificationMode;
      page: EventPage;
    }
  | {
      kind: "policy";
      policy: PolicyVersion;
    }
  | {
      controls: RuntimeControl[];
      kind: "controls";
    }
  | {
      kind: "done";
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
export type Acceptance = "candidate" | "accepted" | "withdrawn";
export type RelationKind =
  "supports" | "challenges" | "conflicts_with" | "derived_from" | "depends_on" | "supersedes" | "triggers";
export type DecisionAnswer = "approve" | "decline";
export type EffectStatus = "confirmed" | "not_performed" | "unknown";
export type WorkStatus = "complete" | "partial" | "blocked";
export type UsageStatus = "known" | "partial" | "unknown";
export type JobState = "queued" | "leased" | "running" | "waiting" | "completed" | "partial" | "failed" | "cancelled";
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
export type ChangeKind = "wording" | "correction" | "world_change" | "support_removal" | "access" | "unresolved";
export type NotificationMode = "material" | "blockers" | "completion" | "muted";
export type ControlKind = "provider" | "family" | "profile";
export type EffectState = "prepared" | "in_progress" | "succeeded" | "failed" | "outcome_unknown";
export type RetentionBoundary = "session" | "upload" | "sandbox" | "provider" | "backup" | "database_storage";
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
export type IntentionState = "pending" | "armed" | "fired" | "completed" | "expired" | "cancelled";
export type Retention = "required" | "optional" | "temporary";
export type Locator =
  | {
      end_line: number;
      kind: "document";
      page?: number | null;
      start_line: number;
    }
  | {
      column?: string | null;
      kind: "table";
      row: number;
    }
  | {
      entity: string;
      kind: "model";
      property: string;
    }
  | {
      kind: "image";
      /**
       * @minItems 4
       * @maxItems 4
       */
      region: [number, number, number, number];
    }
  | {
      end: number;
      kind: "events";
      start: number;
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
export type ArtifactState = "pending" | "uploading" | "ready" | "revoked";
export type SourceKind = "document" | "table" | "model" | "tool_events";
export type UsageKnowledge = "reserved" | "known" | "unknown";

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
export interface MemoryVersion {
  created_by: string;
  decision_id: string;
  record: RecordDraft;
  recorded: CommitPosition;
  recorded_until?: number | null;
  reference: MemoryRef;
  version_id: string;
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
export interface ConfigRef {
  id: string;
  label: string;
  revision: number;
}
export interface MemoryRef {
  label: string;
  memory_id: string;
  revision: number;
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
export interface PolicyDecision {
  action: PolicyAction;
  constraints: string[];
  expires_at?: string | null;
  policy: ConfigRef;
  reason: string;
  required_evidence: string[];
}
export interface CommitPosition {
  recorded_at: string;
  sequence: number;
}
export interface RelationVersion {
  id: string;
  recorded: CommitPosition;
  recorded_until?: number | null;
  relation: RelationDraft;
  revision: number;
  version_id: string;
}
export interface RelationDraft {
  acceptance: Acceptance;
  basis: string;
  evidential_status: EvidentialStatus;
  from: MemoryRef;
  kind: RelationKind;
  scope: Scope;
  to: MemoryRef;
  valid_time: ValidTime;
}
export interface Coverage {
  examined: string[];
  unexamined: string[];
}
export interface DecisionRequest {
  answer?: DecisionAnswer | null;
  answered_by?: string | null;
  deadline: string;
  /**
   * Unanswered or declined requests keep affected execution blocked, including after expiry.
   */
  fallback: string;
  id: string;
  job_id: string;
  missing: string;
  owner_id: string;
  question: string;
  reason?: string | null;
  revision: number;
  scope: Scope;
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
export interface SubmitJob {
  brief: WorkBrief;
  max_attempts: number;
  parent_id?: string | null;
  retain_until: string;
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
export interface Exploration {
  expires_at: string;
  id: string;
  promoted: number[];
  purpose: string;
  records: RecordDraft[];
  revision: number;
  scope: Scope;
}
export interface EventPage {
  cursor: number;
  events: Event[];
  snapshot_required: boolean;
}
export interface Event {
  cursor: number;
  id: string;
  kind: string;
  recorded_at: string;
  resource_id: string;
}
export interface MemoryChangeNotice {
  current: MemoryRef;
  cursor: number;
  id: string;
  kind: ChangeKind;
  previous: MemoryRef;
  reason: string;
  scope: Scope;
  valid_time: ValidTime;
}
export interface PolicyVersion {
  effective: ValidTime;
  policy: MemoryPolicy;
  recorded: CommitPosition;
  reference: ConfigRef;
  scope: Scope;
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
export interface RuntimeControl {
  enabled: boolean;
  kind: ControlKind;
  revision: number;
  target: string;
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
export interface MaintenanceReview {
  affected: SupportReview[];
  before: MemoryVersion;
  conflicts: ConflictReview[];
  coverage: Coverage;
  id: string;
  indirect: MemoryVersion[];
  job_id: string;
  relations: RelationVersion[];
  request: MaintenanceRequest;
}
export interface SupportReview {
  challenges: MemoryVersion[];
  claim: MemoryVersion;
  governed_derivative: boolean;
  remaining: MemoryVersion[];
  source_groups: {
    [k: string]: MemoryRef[];
  };
}
export interface ConflictReview {
  claims: MemoryVersion[];
  relation: RelationVersion;
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
export interface MaintenanceResult {
  changed: MemoryVersion[];
  coverage: Coverage;
  kind: ChangeKind;
  notices: MemoryChangeNotice[];
  preserved: MemoryRef[];
  revalidation: MemoryRef[];
  review_id: string;
  unresolved: string[];
}
export interface ChangePage {
  changes: MemoryChangeNotice[];
  cursor: number;
  snapshot_required: boolean;
}
export interface IntentionCheck {
  event?: Event | null;
  evidence: unknown;
  id: string;
  kind: IntentionCheckKind;
  occurrence: IntentionOccurrence;
}
export interface IntentionOccurrence {
  confirmed_by?: string | null;
  cycle: number;
  definition: MemoryVersion;
  due_at?: string | null;
  event_cursor: number;
  evidence: string[];
  expires_at?: string | null;
  id: string;
  job_id?: string | null;
  previous?: string | null;
  reason?: string | null;
  state: IntentionState;
  trigger_event?: string | null;
  unresolved: string[];
}
export interface IntentionSweep {
  more: boolean;
  occurrences: IntentionOccurrence[];
}
export interface ConsolidationWindow {
  cases: MemoryVersion[];
  cutoff: string;
  exceptions: MemoryRef[];
  id: string;
  job_id: string;
  policy: ConfigRef;
  scope: Scope;
  selection: CohortSelection;
  source_groups: SourceGroup[];
  unexamined: string[];
  unknown_lineage: MemoryRef[];
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
export interface SourceGroup {
  members: MemoryRef[];
  sources: string[];
}
export interface ConsolidationReview {
  id: string;
  proposal: ConsolidationProposal;
  window: ConsolidationWindow;
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
export interface ConsolidationResult {
  coverage: Coverage;
  deferred: string[];
  independent_sources: number;
  record?: MemoryVersion | null;
  review_id: string;
}
export interface AdoptionResult {
  adopted: boolean;
  candidate: MemoryVersion;
  evaluation_job: string;
  rates: {
    [k: string]: number;
  };
  reasons: string[];
  /**
   * Separate evidence for question, model and routing policy; absent/failed criteria remain candidates.
   */
  recognition: {
    [k: string]: RecognitionAdoption;
  };
}
export interface RecognitionAdoption {
  model: boolean;
  policy: boolean;
  question: boolean;
}
export interface EmbeddingInput {
  cursor: SearchCursor;
  memory: MemoryRef;
  text: string;
}
export interface SearchCursor {
  sequence: number;
  version_id: string;
}
export interface ActivationWindow {
  candidates: ActivationCandidate[];
  coverage: string[];
  cursor?: ActivationCursor | null;
  cutoff: number;
  eligible: MemoryRef[];
  groups: ContextGroup[];
  id: string;
  job_id?: string | null;
  next?: ActivationCursor | null;
  /**
   * Lexical updates are transactional; vectors are rechecked against domain versions.
   */
  projection_watermark: number;
  query: ActivationQuery;
}
export interface ActivationCandidate {
  channels: string[];
  /**
   * Each prerequisite/exclusion is judged separately by J08.
   */
  conditions: string[];
  definition?: string | null;
  memory: MemoryVersion;
  score: number;
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
export interface ContextGroup {
  complete: boolean;
  members: MemoryRef[];
  relations: RelationVersion[];
  unresolved_conflict: boolean;
}
export interface ContextPackage {
  deferred: string[];
  dispositions: ActivationDisposition[];
  reason: string;
  selected: MemoryRef[];
  window: ActivationWindow;
}
export interface ActivationDisposition {
  applicability: string;
  memory: MemoryRef;
  missing_conditions: string[];
  roles: string[];
}
export interface FormationWindow {
  after: number;
  cutoff: string;
  entries: FormationEntry[];
  id: string;
  job_id: string;
  operation: string;
  policy: ConfigRef;
  remaining: boolean;
  scope: Scope;
  source: SourceRef;
  through: number;
}
export interface FormationEntry {
  duplicate_records?: MemoryRef[] | null;
  event: ToolEvent;
  input: FormationInput;
  locator: SourceLocator;
  native_locators: SourceLocator[];
  position: number;
  previous_deferral?: DeferredContribution | null;
}
export interface ToolEvent {
  content: unknown;
  event_id: string;
  evidential_status: EvidentialStatus;
  kind: string;
  observed_at: string;
  origin: Origin;
}
/**
 * Supplied by the source connector or explicit contribution UI, not inferred
 * from text claiming to be a user or an observation.
 */
export interface FormationInput {
  actor_id?: string | null;
  based_on?: string[];
  boundary_explicit: boolean;
  conditions: string[];
  content: MemoryContent;
  corrects: string[];
  coverage: Coverage;
  episode: string;
  evidence: unknown;
  explicit_contribution: boolean;
  explicitly_selected: boolean;
  objective: string;
  retention: Retention;
  source_locators: string[];
  uncertainty: string[];
}
export interface SourceLocator {
  id: string;
  locator: Locator;
  source: SourceRef;
}
export interface DeferredContribution {
  event_id: string;
  reason: string;
  required: boolean;
}
export interface FormationResult {
  common_source_groups: SupportGroup[];
  coverage: Coverage;
  cursor: number;
  deferred: DeferredContribution[];
  duplicate_events: {
    [k: string]: MemoryRef[];
  };
  inspected: SourceLocator[];
  judgement_usage: {
    [k: string]: JudgementUsage;
  };
  records: MemoryVersion[];
  unresolved: string[];
  usage: Usage;
  window_id: string;
}
export interface SupportGroup {
  event_ids: string[];
  source: SourceRef;
}
export interface JudgementUsage {
  cost_microunits?: number | null;
  input_tokens?: number | null;
  output_tokens?: number | null;
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
export interface Artifact {
  id: string;
  revision: number;
  spec: ArtifactSpec;
  state: ArtifactState;
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
/**
 * A runtime worker credential. The token is shown only in the issuing response.
 */
export interface IssuedWorkerCredential {
  actor_id: string;
  expires_at: string;
  job_id: string;
  scope: Scope;
  token: string;
}
export interface Assignment {
  epoch: number;
  expires_at: string;
  job: Job;
  owner_id: string;
}
/**
 * What a backup contains and where it stands relative to live state (§20.3).
 */
export interface BackupManifest {
  artifacts: BackupArtifacts;
  created_at: string;
  database: BackupDatabase;
  /**
   * Deletion reports captured in `deletions.json`; restore reapplies them before serving.
   */
  deletions: number;
  host_version: string;
  /**
   * Commit clock sequence at snapshot time; events after it are not in this backup.
   */
  recovery_position: number;
  schema_version: number;
  tenants: string[];
}
export interface BackupArtifacts {
  bytes: number;
  files: number;
  root: string;
}
export interface BackupDatabase {
  bytes: number;
  file: string;
  kind: string;
  sha256: string;
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
export interface BudgetUsage {
  budget: Budget;
  committed: Resources;
  unresolved: Resources;
}
export interface Reservation {
  budget_id: string;
  final_result: boolean;
  id: string;
  job_id: string;
  knowledge: UsageKnowledge;
  maximum: Resources;
  provider_attempt: string;
  usage?: Resources | null;
}
export interface Effect {
  epoch: number;
  id: string;
  job_id: string;
  receipt?: unknown;
  request: EffectRequest;
  state: EffectState;
}
export interface EffectRequest {
  arguments: unknown;
  invocation_id: string;
  kind: string;
  logical_operation_id: string;
  replay: ReplayClass;
}
