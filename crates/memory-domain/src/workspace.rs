//! Working context is persisted by Pi, independently of collection promotion.
use crate::contracts::{
    ConfigRef, EvidentialStatus, Origin, Scope, SourceRef, WireVersion, WorkInputs,
};
use crate::records::ValidTime;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceKind {
    Goal,
    Constraint,
    Observation,
    Hypothesis,
    Plan,
    Obligation,
    Dependency,
    Artifact,
    Memory,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScratchLifetime {
    Task,
    Phase { phase: String },
    Until { expires_at: DateTime<Utc> },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkingStatus {
    Active,
    NeedsRevalidation,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceExposure {
    ParentRead,
    DelegatedFinding,
    NotRead,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceEntry {
    pub id: String,
    pub kind: WorkspaceKind,
    /// A concise, complete statement; large detail stays in referenced artifacts.
    pub text: String,
    pub origin: Origin,
    pub evidential_status: EvidentialStatus,
    pub scope: Scope,
    pub valid_time: ValidTime,
    pub inputs: WorkInputs,
    pub generating_operation: Option<Uuid>,
    pub supporting_entries: Vec<String>,
    pub exposure: EvidenceExposure,
    pub lifetime: ScratchLifetime,
    pub status: WorkingStatus,
    pub decision_relevant: bool,
    pub priority: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConflictGroup {
    pub id: String,
    pub members: Vec<String>,
    pub status: String,
    pub unresolved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceCoverage {
    Examined,
    Unexamined,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SourceInventoryItem {
    pub source: SourceRef,
    pub coverage: SourceCoverage,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContextBundle {
    pub id: String,
    pub members: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_cursor: Option<u32>,
    pub schema_version: WireVersion,
    pub workspace_id: Uuid,
    pub task_id: Uuid,
    pub scope: Scope,
    pub phase: String,
    pub expires_at: DateTime<Utc>,
    /// Recovery retention is not permission to form collection memories.
    pub recovery_until: DateTime<Utc>,
    pub temporary: bool,
    pub selected_contributions: Vec<String>,
    pub entries: Vec<WorkspaceEntry>,
    pub sources: Vec<SourceInventoryItem>,
    pub conflicts: Vec<ConflictGroup>,
    #[serde(default)]
    pub context_groups: Vec<ContextBundle>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeferredContext {
    pub entry_ids: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct RenderUsage {
    pub uncached_input_tokens: Option<u32>,
    pub cache_read_tokens: Option<u32>,
    pub cache_write_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RenderManifest {
    pub schema_version: WireVersion,
    pub decision_id: Uuid,
    pub workspace_id: Uuid,
    pub session_id: String,
    pub lane: String,
    pub operation_id: String,
    pub profile: ConfigRef,
    pub provider: String,
    pub model: String,
    pub access_revision: String,
    pub selected: Vec<WorkspaceEntry>,
    pub sources: Vec<SourceInventoryItem>,
    pub conflicts: Vec<ConflictGroup>,
    pub deferred: Vec<DeferredContext>,
    pub next_step: Option<String>,
    /// The projected transcript and tool schemas, never provider credentials.
    pub messages: Vec<Value>,
    pub tool_schemas: Vec<Value>,
    pub strategy: String,
    pub rebuild_reasons: Vec<String>,
    pub estimator: String,
    pub estimated_input_tokens: u32,
    pub reserved_output_tokens: u32,
    pub reserved_result_tokens: u32,
    pub final_payload_bytes: Option<u32>,
    pub status: String,
    pub usage: RenderUsage,
}
