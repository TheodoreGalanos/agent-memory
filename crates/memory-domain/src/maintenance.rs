use crate::{contracts::*, records::*};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Wording,
    Correction,
    WorldChange,
    SupportRemoval,
    Access,
    Unresolved,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MaintenanceRequest {
    pub before: MemoryRef,
    pub after: Option<RecordDraft>,
    /// Explicit removed source snapshots; the Host verifies their unavailable state.
    pub removed_sources: Vec<SourceRef>,
    /// Bounded retrieval shortlist for indirect dependencies; J16 decides relevance.
    pub candidates: Vec<MemoryRef>,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SupportReview {
    pub claim: MemoryVersion,
    pub remaining: Vec<MemoryVersion>,
    pub source_groups: BTreeMap<Uuid, Vec<MemoryRef>>,
    pub governed_derivative: bool,
    pub challenges: Vec<MemoryVersion>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConflictReview {
    pub relation: RelationVersion,
    pub claims: Vec<MemoryVersion>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MaintenanceReview {
    pub id: Uuid,
    pub job_id: Uuid,
    pub request: MaintenanceRequest,
    pub before: MemoryVersion,
    pub affected: Vec<SupportReview>,
    pub indirect: Vec<MemoryVersion>,
    pub conflicts: Vec<ConflictReview>,
    pub relations: Vec<RelationVersion>,
    pub coverage: Coverage,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MaintenanceCommit {
    pub review_id: Uuid,
    pub decisions: BTreeMap<String, Uuid>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MemoryChangeNotice {
    pub id: Uuid,
    pub cursor: u32,
    pub kind: ChangeKind,
    pub previous: MemoryRef,
    pub current: MemoryRef,
    pub scope: Scope,
    pub valid_time: ValidTime,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MaintenanceResult {
    pub review_id: Uuid,
    pub kind: ChangeKind,
    pub changed: Vec<MemoryVersion>,
    pub preserved: Vec<MemoryRef>,
    pub notices: Vec<MemoryChangeNotice>,
    pub revalidation: Vec<MemoryRef>,
    pub unresolved: Vec<String>,
    pub coverage: Coverage,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChangePage {
    pub changes: Vec<MemoryChangeNotice>,
    pub cursor: u32,
    pub snapshot_required: bool,
}
