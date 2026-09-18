//! Scoped retrieval and context selection. Retrieval never fires intentions.
use crate::{
    contracts::{MemoryRef, Scope},
    records::{MemoryVersion, RelationVersion},
};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EmbeddingIdentity {
    pub model: String,
    pub revision: String,
    pub dimensions: u32,
    pub representation: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Embedding {
    pub identity: EmbeddingIdentity,
    pub values: Vec<f64>,
}
impl Embedding {
    pub fn validate(&self) -> Result<(), String> {
        if self.identity.model.trim().is_empty()
            || self.identity.revision.trim().is_empty()
            || self.identity.representation != "memory-content-v1"
            || !(1..=8192).contains(&self.identity.dimensions)
            || self.values.len() != self.identity.dimensions as usize
            || self.values.iter().any(|v| !v.is_finite() || v.abs() > 1e10)
            || self.values.iter().map(|v| v * v).sum::<f64>() <= 0.0
        {
            return Err("Embedding needs model/revision, memory-content-v1, matching dimensions and a finite nonzero vector".into());
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ActivationQuery {
    pub scope: Scope,
    pub question: String,
    /// Task evidence supplied to semantic applicability checks, not a collection write.
    pub task_context: String,
    pub entities: Vec<Uuid>,
    pub exact: Vec<Uuid>,
    pub families: Vec<String>,
    pub valid_at: Option<DateTime<Utc>>,
    pub recorded_as_of: Option<u32>,
    pub fresh_after: Option<DateTime<Utc>>,
    pub vector: Option<Embedding>,
    pub existing: Vec<MemoryRef>,
    pub scan_limit: u16,
    pub candidate_limit: u16,
    pub traversal_limit: u16,
    pub context_bytes: u32,
}
impl ActivationQuery {
    pub fn validate(&self) -> Result<(), String> {
        if self.question.trim().is_empty()
            || self.question.len() > 2048
            || self.task_context.len() > 8192
            || !(1..=500).contains(&self.scan_limit)
            || !(1..=32).contains(&self.candidate_limit)
            || !(1..=100).contains(&self.traversal_limit)
            || self.context_bytes > 60000
            || self.entities.len() > 32
            || self.exact.len() > 32
            || self.existing.len() > 100
            || self
                .families
                .iter()
                .any(|f| !["episode", "knowledge", "procedure", "intention"].contains(&f.as_str()))
        {
            return Err("Activation query exceeds its bounds or has an unknown family".into());
        }
        if let Some(v) = &self.vector {
            v.validate()?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ActivationCursor {
    pub after_id: Uuid,
    pub cutoff: u32,
    pub query: Box<ActivationQuery>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ActivationCandidate {
    pub memory: MemoryVersion,
    pub score: f64,
    pub channels: Vec<String>,
    /// Each prerequisite/exclusion is judged separately by J08.
    pub conditions: Vec<String>,
    pub definition: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContextGroup {
    pub members: Vec<MemoryRef>,
    pub relations: Vec<RelationVersion>,
    pub unresolved_conflict: bool,
    pub complete: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ActivationWindow {
    pub id: Uuid,
    pub job_id: Option<Uuid>,
    pub query: ActivationQuery,
    pub cursor: Option<ActivationCursor>,
    pub cutoff: u32,
    /// Lexical updates are transactional; vectors are rechecked against domain versions.
    pub projection_watermark: u32,
    pub eligible: Vec<MemoryRef>,
    pub candidates: Vec<ActivationCandidate>,
    pub groups: Vec<ContextGroup>,
    pub next: Option<ActivationCursor>,
    pub coverage: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ActivationSelection {
    pub window_id: Uuid,
    /// candidate index / family / condition index -> WP08 decision ID.
    pub decisions: BTreeMap<String, Uuid>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ActivationDisposition {
    pub memory: MemoryRef,
    pub roles: Vec<String>,
    pub applicability: String,
    pub missing_conditions: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContextPackage {
    pub window: ActivationWindow,
    pub selected: Vec<MemoryRef>,
    pub dispositions: Vec<ActivationDisposition>,
    pub deferred: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchCursor {
    pub sequence: u32,
    pub version_id: Uuid,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EmbeddingInput {
    pub memory: MemoryRef,
    pub text: String,
    pub cursor: SearchCursor,
}
