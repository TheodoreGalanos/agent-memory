//! Durable host coordination; Pi operations remain a separate state machine.
use crate::contracts::{Scope, WorkBrief, WorkResult};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Leased,
    Running,
    Waiting,
    Completed,
    Partial,
    Failed,
    Cancelled,
}
impl JobState {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Partial | Self::Failed | Self::Cancelled
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubmitJob {
    pub brief: WorkBrief,
    pub parent_id: Option<Uuid>,
    pub max_attempts: u16,
    pub retain_until: DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Job {
    pub id: Uuid,
    pub state: JobState,
    pub spec: SubmitJob,
    pub root_id: Uuid,
    pub depth: u16,
    pub attempt: u32,
    pub session_id: String,
    pub operation_id: String,
    pub deadline: DateTime<Utc>,
    pub cancel_requested: bool,
    pub wait_reason: Option<String>,
    pub ready_at: Option<DateTime<Utc>>,
    pub result: Option<WorkResult>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Assignment {
    pub job: Job,
    pub owner_id: Uuid,
    pub epoch: u32,
    pub expires_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Fence {
    pub job_id: Uuid,
    pub owner_id: Uuid,
    pub epoch: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Event {
    pub cursor: u32,
    pub id: Uuid,
    pub resource_id: Uuid,
    pub kind: String,
    pub recorded_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EventPage {
    pub events: Vec<Event>,
    pub cursor: u32,
    pub snapshot_required: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Resources {
    pub tokens: u32,
    pub cost_microunits: u32,
    pub provider_calls: u32,
    pub sandbox_cpu_ms: u32,
    pub sandbox_time_ms: u32,
    pub output_bytes: u32,
}
impl Resources {
    pub fn checked_add(self, other: Self) -> Option<Self> {
        Some(Self {
            tokens: self.tokens.checked_add(other.tokens)?,
            cost_microunits: self.cost_microunits.checked_add(other.cost_microunits)?,
            provider_calls: self.provider_calls.checked_add(other.provider_calls)?,
            sandbox_cpu_ms: self.sandbox_cpu_ms.checked_add(other.sandbox_cpu_ms)?,
            sandbox_time_ms: self.sandbox_time_ms.checked_add(other.sandbox_time_ms)?,
            output_bytes: self.output_bytes.checked_add(other.output_bytes)?,
        })
    }
    pub fn fits(self, limit: Self) -> bool {
        self.tokens <= limit.tokens
            && self.cost_microunits <= limit.cost_microunits
            && self.provider_calls <= limit.provider_calls
            && self.sandbox_cpu_ms <= limit.sandbox_cpu_ms
            && self.sandbox_time_ms <= limit.sandbox_time_ms
            && self.output_bytes <= limit.output_bytes
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Budget {
    pub id: Uuid,
    pub scope: Scope,
    pub limit: Resources,
    pub final_result_reserve: Resources,
    pub deadline: DateTime<Utc>,
    pub max_child_depth: u16,
    pub max_child_concurrency: u16,
    pub pricing_revision: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UsageKnowledge {
    Reserved,
    Known,
    Unknown,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Reservation {
    pub id: Uuid,
    pub job_id: Uuid,
    pub budget_id: Uuid,
    pub provider_attempt: String,
    pub maximum: Resources,
    pub usage: Option<Resources>,
    pub knowledge: UsageKnowledge,
    pub final_result: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BudgetUsage {
    pub budget: Budget,
    pub committed: Resources,
    pub unresolved: Resources,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReplayClass {
    Observation,
    RemoteIdempotent,
    Reconcile,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EffectState {
    Prepared,
    InProgress,
    Succeeded,
    Failed,
    OutcomeUnknown,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EffectRequest {
    pub logical_operation_id: String,
    pub kind: String,
    pub invocation_id: String,
    pub replay: ReplayClass,
    pub arguments: serde_json::Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Effect {
    pub id: Uuid,
    pub job_id: Uuid,
    pub request: EffectRequest,
    pub state: EffectState,
    pub epoch: u32,
    pub receipt: Option<serde_json::Value>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EffectResolution {
    Succeeded,
    Failed,
    NotPerformed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MemoryCommit {
    pub fence: Fence,
    pub changes: Vec<crate::records::MemoryChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum HostRequest {
    User {
        request: Box<crate::interaction::UserRequest>,
    },
    RecordContext {
        fence: Fence,
        manifest: Box<crate::workspace::RenderManifest>,
    },
    CheckProvider {
        fence: Fence,
        provider: String,
    },
    ListDeletions {
        after_id: Option<Uuid>,
        limit: u16,
    },
    ReconcileDeletedEffect {
        deletion: Uuid,
        effect_id: Uuid,
        resolution: EffectResolution,
        evidence_artifact: Uuid,
    },
    ContinueDeletedWork {
        deletion: Uuid,
        old_job: Uuid,
        command: Box<crate::contracts::Command<SubmitJob>>,
    },
    BeginDeletion {
        request: crate::retention::DeletionRequest,
    },
    InspectDeletion {
        id: Uuid,
    },
    PurgeDeletion {
        id: Uuid,
    },
    AcknowledgePurge {
        id: Uuid,
        boundary: crate::retention::RetentionBoundary,
        container: String,
    },
    RestoreDeletionRegistry {
        reports: Vec<crate::retention::DeletionReport>,
    },
    ReviewMaintenance {
        fence: Fence,
        id: Uuid,
        request: Box<crate::maintenance::MaintenanceRequest>,
    },
    CommitMaintenance {
        fence: Fence,
        request: crate::maintenance::MaintenanceCommit,
    },
    MemoryChanges {
        fence: Fence,
        after: u32,
        limit: u16,
    },
    CurrentMemories {
        fence: Fence,
        references: Vec<crate::contracts::MemoryRef>,
    },
    IntentionCheck {
        fence: Fence,
        id: Uuid,
        occurrence_id: Uuid,
        kind: crate::intentions::IntentionCheckKind,
    },
    ApplyIntentionCheck {
        fence: Fence,
        check_id: Uuid,
        decisions: std::collections::BTreeMap<String, Uuid>,
    },
    InspectIntentions {
        definition_id: Uuid,
    },
    CancelIntention {
        occurrence_id: Uuid,
        reason: String,
    },
    ConfirmIntention {
        occurrence_id: Uuid,
    },
    SweepIntentions {
        limit: u16,
    },
    ConsolidationWindow {
        fence: Fence,
        id: Uuid,
        selection: crate::consolidation::CohortSelection,
    },
    ReviewConsolidation {
        fence: Fence,
        id: Uuid,
        proposal: Box<crate::consolidation::ConsolidationProposal>,
    },
    CommitConsolidation {
        fence: Fence,
        request: crate::consolidation::ConsolidationCommit,
    },
    StartQualification {
        fence: Fence,
        request_id: Uuid,
        review_id: Uuid,
        suite_id: Uuid,
    },
    AdoptProcedure {
        fence: Fence,
        evaluation_job: Uuid,
    },
    EmbeddingInputs {
        after: Option<crate::activation::SearchCursor>,
        limit: u16,
    },
    Activate {
        fence: Fence,
        id: Uuid,
        query: Box<crate::activation::ActivationQuery>,
        cursor: Option<crate::activation::ActivationCursor>,
    },
    SelectActivation {
        fence: Fence,
        selection: crate::activation::ActivationSelection,
    },
    SaveEmbedding {
        reference: crate::contracts::MemoryRef,
        embedding: crate::activation::Embedding,
    },
    FormationWindow {
        fence: Fence,
        id: Uuid,
        source: crate::contracts::SourceRef,
        operation: String,
        limit: u32,
    },
    CommitFormation {
        fence: Fence,
        request: Box<crate::formation::FormationCommit>,
    },
    AdmitJudgement {
        fence: Fence,
        packet: Box<crate::judgement::JudgementPacket>,
    },
    CheckJudgement {
        fence: Fence,
        packet_id: Uuid,
        provider: String,
    },
    RecordAssessment {
        fence: Fence,
        assessment: Box<crate::judgement::SemanticAssessment>,
    },
    ReuseAssessment {
        fence: Fence,
        assessment_id: Uuid,
        packet_id: Uuid,
        provider: String,
        model_release: String,
    },
    RecordJudgementDecision {
        fence: Fence,
        decision: Box<crate::judgement::JudgementDecision>,
    },
    RegisterTaskCheck {
        fence: Fence,
        check: Box<crate::judgement::TaskLocalCheck>,
    },
    InspectTaskCheck {
        fence: Fence,
        id: Uuid,
    },
    RetireTaskCheck {
        fence: Fence,
        id: Uuid,
    },
    CreateBudget {
        budget: Box<Budget>,
    },
    Submit {
        command: Box<crate::contracts::Command<SubmitJob>>,
    },
    SpawnChild {
        fence: Fence,
        request_id: Uuid,
        brief: Box<WorkBrief>,
        deadline: DateTime<Utc>,
        reuse_job_id: Option<Uuid>,
    },
    ChildJobs {
        fence: Fence,
        ids: Vec<Uuid>,
    },
    WaitChildren {
        fence: Fence,
        ids: Vec<Uuid>,
        ready_at: DateTime<Utc>,
    },
    ReadInput {
        fence: Fence,
        artifact_id: Uuid,
        offset: u32,
        limit: u32,
    },
    PublishArtifact {
        fence: Fence,
        request_id: Uuid,
        label: String,
        text: String,
        dependencies: Vec<Uuid>,
    },
    InspectJob {
        job_id: Uuid,
    },
    /// An administrator writes a consistent backup into a new directory (local SQLite profile).
    Backup {
        directory: std::path::PathBuf,
    },
    /// A worker pool claims the oldest eligible queued job whose process it names, within its
    /// work classes, and receives a job-bound worker credential with the assignment.
    ClaimNext {
        processes: Vec<crate::contracts::Process>,
        lease_seconds: u16,
    },
    /// An administrator mints a worker credential bound to one job, its brief scope and
    /// its deadline. The token is returned once and held by the running host only.
    IssueWorkerCredential {
        job_id: Uuid,
    },
    /// A trusted connector publishes one source revision. A JSON string is stored as
    /// text; any other value is serialized JSON. The adapter follows `source.kind`.
    IngestSource {
        source: crate::sources::SourceVersion,
        content: serde_json::Value,
    },
    Claim {
        job_id: Uuid,
        lease_seconds: u16,
    },
    Start {
        fence: Fence,
    },
    InspectAssignment {
        fence: Fence,
    },
    Renew {
        fence: Fence,
        lease_seconds: u16,
    },
    Wait {
        fence: Fence,
        reason: String,
        ready_at: DateTime<Utc>,
    },
    Cancel {
        job_id: Uuid,
    },
    AcknowledgeCancellation {
        fence: Fence,
    },
    Complete {
        request_id: Uuid,
        fence: Fence,
        result: Box<WorkResult>,
    },
    CommitMemories {
        command: Box<crate::contracts::Command<MemoryCommit>>,
    },
    Reserve {
        fence: Fence,
        provider_attempt: String,
        maximum: Resources,
        final_result: bool,
    },
    SettleUsage {
        fence: Fence,
        reservation_id: Uuid,
        observed: Option<Resources>,
    },
    BudgetUsage {
        budget_id: Uuid,
    },
    PrepareEffect {
        fence: Fence,
        request: EffectRequest,
    },
    BeginEffect {
        fence: Fence,
        effect_id: Uuid,
    },
    ReportEffect {
        fence: Fence,
        effect_id: Uuid,
        state: EffectState,
        receipt: serde_json::Value,
    },
    InspectEffect {
        effect_id: Uuid,
    },
    ReconcileEffect {
        effect_id: Uuid,
        resolution: EffectResolution,
        evidence: serde_json::Value,
    },
    Events {
        after: u32,
        limit: u16,
    },
    ConsumeEvent {
        consumer: String,
        event_id: Uuid,
        command: Box<crate::contracts::Command<SubmitJob>>,
    },
    AllocateArtifact {
        id: Uuid,
        spec: Box<crate::sources::ArtifactSpec>,
    },
    RecoverArtifactUpload {
        id: Uuid,
        revision: u32,
    },
    Recover,
    PruneHistory,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HostResponse {
    User {
        result: Box<crate::interaction::UserResponse>,
    },
    Deletions {
        reports: Vec<crate::retention::DeletionReport>,
    },
    Deletion {
        report: crate::retention::DeletionReport,
    },
    MaintenanceReview {
        review: Box<crate::maintenance::MaintenanceReview>,
    },
    MaintenanceResult {
        result: Box<crate::maintenance::MaintenanceResult>,
    },
    Changes {
        page: crate::maintenance::ChangePage,
    },
    IntentionCheck {
        check: Box<crate::intentions::IntentionCheck>,
    },
    Intention {
        occurrence: Box<crate::intentions::IntentionOccurrence>,
    },
    Intentions {
        occurrences: Vec<crate::intentions::IntentionOccurrence>,
    },
    IntentionSweep {
        result: crate::intentions::IntentionSweep,
    },
    ConsolidationWindow {
        window: Box<crate::consolidation::ConsolidationWindow>,
    },
    ConsolidationReview {
        review: Box<crate::consolidation::ConsolidationReview>,
    },
    ConsolidationResult {
        result: Box<crate::consolidation::ConsolidationResult>,
    },
    AdoptionResult {
        result: Box<crate::consolidation::AdoptionResult>,
    },
    EmbeddingInputs {
        inputs: Vec<crate::activation::EmbeddingInput>,
    },
    ActivationWindow {
        window: Box<crate::activation::ActivationWindow>,
    },
    ContextPackage {
        package: Box<crate::activation::ContextPackage>,
    },
    FormationWindow {
        window: Box<crate::formation::FormationWindow>,
    },
    FormationResult {
        result: Box<crate::formation::FormationResult>,
    },
    JudgementPacket {
        packet: Box<crate::judgement::JudgementPacket>,
    },
    Assessment {
        assessment: Option<Box<crate::judgement::SemanticAssessment>>,
    },
    JudgementDecision {
        decision: Box<crate::judgement::JudgementDecision>,
    },
    TaskCheck {
        check: Box<crate::judgement::TaskLocalCheck>,
    },
    ArtifactData {
        text: String,
    },
    Children {
        jobs: Vec<Job>,
    },
    Artifact {
        artifact: Box<crate::sources::Artifact>,
    },
    Source {
        source: Box<crate::sources::SourceVersion>,
    },
    WorkerCredential {
        credential: Box<IssuedWorkerCredential>,
    },
    Work {
        assignment: Box<Assignment>,
        credential: Box<IssuedWorkerCredential>,
    },
    Backup {
        manifest: Box<BackupManifest>,
    },
    /// No eligible job for the requested processes.
    Idle,
    Job {
        job: Box<Job>,
    },
    Assignment {
        assignment: Box<Assignment>,
    },
    Budget {
        budget: Box<Budget>,
    },
    BudgetUsage {
        usage: Box<BudgetUsage>,
    },
    Reservation {
        reservation: Reservation,
    },
    Effect {
        effect: Effect,
    },
    Memories {
        memories: Vec<crate::records::MemoryVersion>,
    },
    Events {
        page: EventPage,
    },
    Done {
        affected: u64,
    },
}

/// A runtime worker credential. The token is shown only in the issuing response.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IssuedWorkerCredential {
    pub token: String,
    pub job_id: Uuid,
    pub actor_id: Uuid,
    pub scope: Scope,
    pub expires_at: DateTime<Utc>,
}

/// Capacity classes from the deployment profile: interactive work answers a waiting task;
/// deferred work distils and maintains memory. Pools are granted classes, not processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkClass {
    Interactive,
    Deferred,
}
impl WorkClass {
    pub fn of(process: crate::contracts::Process) -> Self {
        use crate::contracts::Process as P;
        match process {
            P::Activation | P::Investigation => Self::Interactive,
            P::Formation | P::Consolidation | P::Maintenance | P::Evaluation => Self::Deferred,
        }
    }
}

/// What a backup contains and where it stands relative to live state (§20.3).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BackupManifest {
    pub created_at: DateTime<Utc>,
    pub host_version: String,
    pub database: BackupDatabase,
    /// Commit clock sequence at snapshot time; events after it are not in this backup.
    pub recovery_position: u64,
    pub schema_version: u64,
    pub artifacts: BackupArtifacts,
    /// Deletion reports captured in `deletions.json`; restore reapplies them before serving.
    pub deletions: u32,
    pub tenants: Vec<Uuid>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BackupDatabase {
    pub kind: String,
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BackupArtifacts {
    pub root: String,
    pub files: u64,
    pub bytes: u64,
}
