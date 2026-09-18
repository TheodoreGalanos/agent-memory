use crate::{
    contracts::{ConfigRef, Scope, WorkInputs},
    records::PolicyDecision,
};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QuestionForm {
    Choice { criteria: BTreeMap<String, String> },
    Noul { criteria: BTreeMap<String, String> },
    Score { criteria: Vec<String> },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct JudgementQuestion {
    pub key: String,
    pub instructions: String,
    pub form: QuestionForm,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct JudgementDefinition {
    pub id: String,
    pub revision: u32,
    pub owner: String,
    pub input_requirements: Vec<String>,
    pub questions: Vec<JudgementQuestion>,
    pub applicability: String,
    pub permitted_uses: Vec<String>,
    pub evaluation_refs: Vec<String>,
    pub fallback: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PacketQuestion {
    pub definition: JudgementDefinition,
    pub disclosure_scope: Scope,
    pub allowed_providers: Vec<String>,
    /// Named evidence fields containing results from earlier packets.
    pub depends_on: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct JudgementEvidence {
    pub name: String,
    pub artifact_id: Uuid,
    pub pointer: String,
    pub content: serde_json::Value,
    pub coverage: Vec<String>,
    pub origin: crate::contracts::Origin,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct JudgementPacket {
    pub id: Uuid,
    pub job_id: Uuid,
    pub subject: String,
    pub frame: String,
    pub scope: Scope,
    pub inputs: WorkInputs,
    pub evidence: Vec<JudgementEvidence>,
    pub missing: Vec<String>,
    pub questions: Vec<PacketQuestion>,
    pub disclosure_policy: String,
    pub allowed_providers: Vec<String>,
    pub policy: ConfigRef,
    pub budget_id: Uuid,
    pub evidence_cutoff: DateTime<Utc>,
    pub fresh_after: DateTime<Utc>,
    pub deadline: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub local_check_id: Option<Uuid>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum JudgementAnswer {
    Choice {
        choice: String,
        probabilities: Option<BTreeMap<String, f64>>,
        confidence: Option<f64>,
    },
    Noul {
        noul: f64,
    },
    Score {
        score: f64,
        legend: BTreeMap<String, String>,
        probabilities: Option<BTreeMap<String, f64>>,
        confidence: Option<f64>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssessmentStatus {
    Answered,
    InvalidResponse,
    Unavailable,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct JudgementUsage {
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub cost_microunits: Option<u32>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SemanticAssessment {
    pub id: Uuid,
    pub packet_id: Uuid,
    pub raw_artifact_id: Uuid,
    pub reservation_id: Uuid,
    pub provider: String,
    pub requested_model: String,
    pub returned_model: Option<String>,
    pub model_release: Option<String>,
    pub status: AssessmentStatus,
    pub answers: BTreeMap<String, JudgementAnswer>,
    pub failure: Option<String>,
    pub usage: JudgementUsage,
    pub assessed_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct JudgementDecision {
    pub id: Uuid,
    pub packet_id: Uuid,
    pub assessment_ids: Vec<Uuid>,
    pub selected_assessment: Option<Uuid>,
    pub decisions: BTreeMap<String, PolicyDecision>,
    pub inconsistencies: Vec<String>,
    pub mode: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TaskLocalCheck {
    pub id: Uuid,
    pub job_id: Uuid,
    pub task_id: Uuid,
    pub definition: JudgementDefinition,
    pub scope: Scope,
    pub allowed_providers: Vec<String>,
    pub disclosure_policy: String,
    pub completion_requirements: Vec<String>,
    pub expires_at: DateTime<Utc>,
    pub retired: bool,
}

impl JudgementDefinition {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty()
            || self.revision == 0
            || self.owner.trim().is_empty()
            || self.questions.is_empty()
            || self.input_requirements.is_empty()
            || self.permitted_uses.is_empty()
            || self.evaluation_refs.is_empty()
            || self.questions.len() > 16
            || self.fallback.trim().is_empty()
            || self.applicability.trim().is_empty()
        {
            return Err("Incomplete judgement definition".into());
        }
        let mut keys = std::collections::HashSet::new();
        for q in &self.questions {
            if q.key.is_empty() || !keys.insert(&q.key) || q.instructions.trim().is_empty() {
                return Err("Questions need unique keys and full instructions".into());
            }
            match &q.form {
                QuestionForm::Choice { criteria }
                    if criteria.len() < 2 || criteria.values().any(|v| v.trim().is_empty()) =>
                {
                    return Err("Choice needs described alternatives".into());
                }
                QuestionForm::Noul { criteria }
                    if criteria.keys().map(String::as_str).collect::<Vec<_>>()
                        != ["false", "true"] =>
                {
                    return Err("Noul needs true and false meanings".into());
                }
                QuestionForm::Score { criteria }
                    if criteria.len() < 2 || criteria.iter().any(|v| v.trim().is_empty()) =>
                {
                    return Err("Score needs at least two described levels".into());
                }
                _ => {}
            }
        }
        Ok(())
    }
}
impl JudgementPacket {
    pub fn validate(&self) -> Result<(), String> {
        if self.subject.trim().is_empty()
            || self.frame.trim().is_empty()
            || self.questions.is_empty()
            || self.questions.len() > 32
            || self.evidence.is_empty()
            || self.deadline > self.expires_at
            || self.fresh_after > self.deadline
        {
            return Err("Invalid or unbounded judgement packet".into());
        }
        let mut ids = std::collections::HashSet::new();
        for q in &self.questions {
            q.definition.validate()?;
            if !ids.insert(&q.definition.id)
                || !q.disclosure_scope.permits(&self.scope)
                || self
                    .allowed_providers
                    .iter()
                    .any(|p| !q.allowed_providers.contains(p))
            {
                return Err("Question disclosure does not cover the shared packet".into());
            }
        }
        if self
            .questions
            .iter()
            .any(|q| q.depends_on.iter().any(|id| ids.contains(id)))
        {
            return Err("Dependent judgements need separate packets".into());
        }
        let mut names = std::collections::HashSet::new();
        for e in &self.evidence {
            if e.name.trim().is_empty()
                || !names.insert(&e.name)
                || !self.inputs.artifacts.contains(&e.artifact_id)
                || e.coverage.is_empty()
            {
                return Err("Evidence needs content, provenance and coverage".into());
            }
        }
        for question in &self.questions {
            if question.depends_on.iter().any(|name| !names.contains(name))
                || question
                    .definition
                    .input_requirements
                    .iter()
                    .any(|name| !names.contains(name) && !self.missing.contains(name))
            {
                return Err("Required fields need inspected evidence or an explicit missing marker; dependencies need earlier results".into());
            }
        }
        Ok(())
    }
}

/// Tolerance is for transport rounding, not a confidence or acceptance threshold.
pub const DISTRIBUTION_TOLERANCE: f64 = 0.001;
/// Allow rounded totals while preserving the returned probabilities.
pub const DISTRIBUTION_SUM_TOLERANCE: f64 = 0.01;
impl SemanticAssessment {
    pub fn validate(&self, packet: &JudgementPacket) -> Result<(), String> {
        if self.packet_id != packet.id
            || self.provider.is_empty()
            || self.requested_model.is_empty()
            || self
                .model_release
                .as_ref()
                .is_some_and(|release| Some(release) != self.returned_model.as_ref())
            || self.expires_at > packet.expires_at
            || self.assessed_at > self.expires_at
        {
            return Err("Assessment does not match its packet".into());
        }
        if self.status != AssessmentStatus::Answered {
            return if self.answers.is_empty()
                && self.failure.as_ref().is_some_and(|v| !v.is_empty())
            {
                Ok(())
            } else {
                Err("Failed responses cannot contain assessed answers".into())
            };
        }
        if self.returned_model.as_ref().is_none_or(|v| v.is_empty()) || self.failure.is_some() {
            return Err("An answer needs the returned model and no service failure".into());
        }
        let expected: BTreeMap<_, _> = packet
            .questions
            .iter()
            .flat_map(|d| {
                d.definition
                    .questions
                    .iter()
                    .map(move |q| (format!("{}.{}", d.definition.id, q.key), q))
            })
            .collect();
        if expected.keys().ne(self.answers.keys()) {
            return Err("Answer keys differ from the packet questions".into());
        }
        for (key, q) in expected {
            let invalid = || "Answer does not match its declared form".to_string();
            match (&q.form, &self.answers[&key]) {
                (QuestionForm::Noul { .. }, JudgementAnswer::Noul { noul })
                    if probability(*noul) => {}
                (
                    QuestionForm::Choice { criteria },
                    JudgementAnswer::Choice {
                        choice,
                        probabilities,
                        confidence,
                    },
                ) if criteria.contains_key(choice) => {
                    distribution(
                        probabilities,
                        confidence,
                        criteria.keys().cloned().collect(),
                    )?;
                    if let Some(p) = probabilities
                        && p.values().any(|v| *v > p[choice] + DISTRIBUTION_TOLERANCE)
                    {
                        return Err(invalid());
                    }
                }
                (
                    QuestionForm::Score { criteria },
                    JudgementAnswer::Score {
                        score,
                        legend,
                        probabilities,
                        confidence,
                    },
                ) => {
                    let levels: BTreeMap<_, _> = criteria
                        .iter()
                        .enumerate()
                        .map(|(i, v)| (i.to_string(), v.clone()))
                        .collect();
                    if !score.is_finite()
                        || *score < 0.0
                        || *score > (criteria.len() - 1) as f64
                        || legend != &levels
                    {
                        return Err(invalid());
                    }
                    distribution(probabilities, confidence, levels.keys().cloned().collect())?;
                    if let Some(p) = probabilities {
                        let mean: f64 = p.iter().map(|(k, v)| k.parse::<f64>().unwrap() * v).sum();
                        if (mean - score).abs() > DISTRIBUTION_TOLERANCE {
                            return Err(invalid());
                        }
                    }
                }
                _ => return Err(invalid()),
            }
        }
        Ok(())
    }
}
fn probability(n: f64) -> bool {
    n.is_finite() && (0.0..=1.0).contains(&n)
}
fn distribution(
    values: &Option<BTreeMap<String, f64>>,
    confidence: &Option<f64>,
    keys: Vec<String>,
) -> Result<(), String> {
    match (values, confidence) {
        (None, None) => Ok(()),
        (Some(p), Some(c))
            if p.keys().cloned().collect::<Vec<_>>() == keys
                && p.values().all(|v| probability(*v))
                && probability(*c)
                && (p.values().sum::<f64>() - 1.0).abs()
                    <= DISTRIBUTION_SUM_TOLERANCE + f64::EPSILON * p.len() as f64 =>
        {
            Ok(())
        }
        _ => Err("Invalid probability distribution or confidence".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distribution_allows_one_percent_sum_rounding() {
        for (sum, accepted) in [
            (0.99, true),
            (1.0, true),
            (1.01, true),
            (0.989999, false),
            (1.010001, false),
            (0.98, false),
            (1.02, false),
        ] {
            let values = BTreeMap::from([("a".into(), 0.9), ("b".into(), sum - 0.9)]);
            assert_eq!(
                distribution(&Some(values), &Some(0.8), vec!["a".into(), "b".into()]).is_ok(),
                accepted,
                "sum {sum}"
            );
        }
    }

    #[test]
    fn sum_rounding_does_not_allow_invalid_probability_values() {
        for values in [[1.005, -0.005], [f64::NAN, 0.0], [f64::INFINITY, 0.0]] {
            let values = BTreeMap::from([("a".into(), values[0]), ("b".into(), values[1])]);
            assert!(distribution(&Some(values), &Some(0.8), vec!["a".into(), "b".into()]).is_err());
        }
    }
}
