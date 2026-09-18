//! Bounded synthesis and evidence-based adoption of reusable methods.
use crate::{contracts::*, coordination::ReplayClass, judgement::JudgementDefinition, records::*};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConsolidationPolicy {
    pub min_independent_sources: u16,
    pub min_held_out_groups: u16,
    pub minimum_success_rate: f64,
    pub maximum_regression: f64,
    pub maximum_cost_microunits: u32,
    pub evaluator_profile: ConfigRef,
}
impl ConsolidationPolicy {
    pub fn validate(&self) -> Result<(), String> {
        if self.min_independent_sources == 0
            || self.min_held_out_groups == 0
            || !self.minimum_success_rate.is_finite()
            || !(0.0..=1.0).contains(&self.minimum_success_rate)
            || !self.maximum_regression.is_finite()
            || !(0.0..=1.0).contains(&self.maximum_regression)
        {
            return Err(
                "Consolidation needs positive source/holdout counts and rates in [0,1]".into(),
            );
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CohortSelection {
    pub purpose: String,
    pub mechanism: String,
    pub outcomes: Vec<String>,
    pub source_context: String,
    /// Explicit shortlist obtained from scoped retrieval; linked exceptions are added by the host.
    pub members: Vec<MemoryRef>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SourceGroup {
    pub sources: Vec<Uuid>,
    pub members: Vec<MemoryRef>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConsolidationWindow {
    pub id: Uuid,
    pub job_id: Uuid,
    pub selection: CohortSelection,
    pub scope: Scope,
    pub policy: ConfigRef,
    pub cutoff: DateTime<Utc>,
    pub cases: Vec<MemoryVersion>,
    pub source_groups: Vec<SourceGroup>,
    pub unknown_lineage: Vec<MemoryRef>,
    pub exceptions: Vec<MemoryRef>,
    pub unexamined: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConditionalClause {
    pub text: String,
    pub conditions: Vec<String>,
    pub uncertainty: Vec<String>,
    pub support: Vec<MemoryRef>,
    pub exceptions: Vec<MemoryRef>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RecognitionCriterion {
    pub definition: JudgementDefinition,
    pub model: String,
    pub model_release: String,
    pub policy: ConfigRef,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProcedureContract {
    pub parameters: BTreeMap<String, String>,
    pub effects: Vec<String>,
    pub replay_class: ReplayClass,
    pub decision_points: Vec<String>,
    pub stopping_criteria: Vec<String>,
    pub checks: Vec<String>,
    pub recognition: Vec<RecognitionCriterion>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConsolidationProposal {
    pub window_id: Uuid,
    pub label: String,
    pub scope: Scope,
    pub concise_summary: String,
    pub invariant: String,
    pub variations: Vec<String>,
    pub untested: Vec<String>,
    pub clauses: Vec<ConditionalClause>,
    /// Omit for conditional knowledge. Procedure content includes its execution/investigation contract.
    pub procedure: Option<MemoryContent>,
}
impl ConsolidationProposal {
    pub fn validate(&self) -> Result<(), String> {
        if self.label.trim().is_empty()
            || self.concise_summary.trim().is_empty()
            || self.invariant.trim().is_empty()
            || self.clauses.is_empty()
            || self.clauses.len() > 16
            || self
                .clauses
                .iter()
                .any(|c| c.text.trim().is_empty() || c.support.is_empty())
        {
            return Err(
                "Synthesis needs a summary, invariant and 1 to 16 supported clauses".into(),
            );
        }
        if let Some(content) = &self.procedure {
            content.validate()?;
            match content {
                MemoryContent::Procedure {
                    contract: Some(c),
                    applicability,
                    ..
                } if !applicability.is_empty()
                    && !c.checks.is_empty()
                    && !c.stopping_criteria.is_empty() => {}
                _ => return Err(
                    "Reusable procedures need conditions, a contract, checks and stopping criteria"
                        .into(),
                ),
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConsolidationReview {
    pub id: Uuid,
    pub window: ConsolidationWindow,
    pub proposal: ConsolidationProposal,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConsolidationCommit {
    pub review_id: Uuid,
    /// J11 for the cohort and J12 for the complete proposal and each clause.
    pub decisions: BTreeMap<String, Uuid>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConsolidationResult {
    pub review_id: Uuid,
    pub record: Option<MemoryVersion>,
    pub deferred: Vec<String>,
    pub independent_sources: u16,
    pub coverage: Coverage,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QualificationCase {
    pub id: String,
    /// All variants/restatements of a source must share this group.
    pub source_group: Uuid,
    pub conditions: Vec<String>,
    pub task: serde_json::Value,
    pub expected: serde_json::Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QualificationSuite {
    pub evaluator_profile: ConfigRef,
    pub cases: Vec<QualificationCase>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QualificationArm {
    Episodes,
    Summary,
    Procedure,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RecognitionObservation {
    pub criterion: String,
    pub packet_valid: bool,
    pub answer_correct: bool,
    pub routing_correct: bool,
    pub false_negative: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QualificationTrial {
    pub case_id: String,
    pub arm: QualificationArm,
    pub answer: serde_json::Value,
    pub evidence_artifact: Uuid,
    pub cost_microunits: u32,
    pub recognition: Vec<RecognitionObservation>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QualificationReport {
    pub review_id: Uuid,
    pub candidate: MemoryRef,
    pub suite_id: Uuid,
    pub evaluator_profile: ConfigRef,
    pub trials: Vec<QualificationTrial>,
    pub unresolved: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AdoptionResult {
    pub evaluation_job: Uuid,
    pub candidate: MemoryVersion,
    pub adopted: bool,
    pub reasons: Vec<String>,
    pub rates: BTreeMap<String, f64>,
    /// Separate evidence for question, model and routing policy; absent/failed criteria remain candidates.
    pub recognition: BTreeMap<String, RecognitionAdoption>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RecognitionAdoption {
    pub question: bool,
    pub model: bool,
    pub policy: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QualificationInput {
    pub review: ConsolidationReview,
    pub candidate: MemoryVersion,
    pub suite_id: Uuid,
}
