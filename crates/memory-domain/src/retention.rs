//! Deletion reports separate access denial, live payload removal and external retention.
use crate::contracts::Scope;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeletionRequest {
    pub id: Uuid,
    /// Resource identities: memories, artifacts, jobs, sources, or source-version scope IDs.
    /// A source identity is deleted across its retained revisions.
    pub resources: Vec<Uuid>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeletionReport {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub epoch: u32,
    pub requested_resources: Vec<Uuid>,
    pub revoked_ids: Vec<Uuid>,
    pub scope: Scope,
    pub resources: Vec<Uuid>,
    pub support_review: Vec<Uuid>,
    pub sessions: Vec<String>,
    pub effects: Vec<DeletedEffect>,
    pub access_denied: bool,
    pub live_payloads_removed: bool,
    pub obligations: Vec<PurgeObligation>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PurgeObligation {
    pub container: String,
    pub kind: RetentionBoundary,
    pub acknowledged: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RetentionBoundary {
    Session,
    Upload,
    Sandbox,
    Provider,
    Backup,
    DatabaseStorage,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeletedEffect {
    pub id: Uuid,
    pub state: crate::coordination::EffectState,
    pub evidence_artifact: Option<Uuid>,
}
