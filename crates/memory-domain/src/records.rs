use crate::contracts::{ConfigRef, EvidentialStatus, MemoryRef, Origin, Scope};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ValidTime {
    Unknown,
    Interval {
        from: Option<DateTime<Utc>>,
        to: Option<DateTime<Utc>>,
    },
}

impl ValidTime {
    pub fn validate(&self) -> Result<(), String> {
        if let Self::Interval {
            from: Some(from),
            to: Some(to),
        } = self
            && from >= to
        {
            return Err("Valid time must be a non-empty half-open interval".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Routine,
    Historical,
    Retired,
    Quarantined,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Qualification {
    Candidate,
    Evaluated {
        conditions: Vec<String>,
        evidence: Vec<MemoryRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        evaluation_artifact: Option<Uuid>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        evaluator_profile: Option<ConfigRef>,
    },
    Withdrawn {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "form", rename_all = "snake_case")]
pub enum ProcedureForm {
    Advisory {
        steps: Vec<String>,
        evidence_criteria: Vec<String>,
    },
    Executable {
        artifact_id: Uuid,
        entrypoint: String,
        inputs: Vec<String>,
        outputs: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "family", rename_all = "snake_case")]
pub enum MemoryContent {
    Episode {
        objective: String,
        initial_conditions: Vec<String>,
        observations: Vec<String>,
        actions: Vec<String>,
        corrections: Vec<String>,
        outcome: String,
        verification: Vec<String>,
        uncertainty: Vec<String>,
    },
    Knowledge {
        statement: String,
        subject: Option<Uuid>,
        predicate: Option<String>,
        uncertainty: Vec<String>,
        examined_coverage: Vec<String>,
    },
    Procedure {
        purpose: String,
        capabilities: Vec<String>,
        applicability: Vec<String>,
        exclusions: Vec<String>,
        method: ProcedureForm,
        counterexamples: Vec<MemoryRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        contract: Option<crate::consolidation::ProcedureContract>,
    },
    Intention {
        purpose: String,
        owner_id: Uuid,
        trigger: String,
        readiness: Vec<String>,
        completion: Vec<String>,
        expires_at: Option<DateTime<Utc>>,
        notification_policy: String,
        recurrence: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        plan: Option<Box<crate::intentions::IntentionPlan>>,
    },
}

impl MemoryContent {
    pub fn family(&self) -> &'static str {
        match self {
            Self::Episode { .. } => "episode",
            Self::Knowledge { .. } => "knowledge",
            Self::Procedure { .. } => "procedure",
            Self::Intention { .. } => "intention",
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let summary = match self {
            Self::Episode { objective, .. } => objective,
            Self::Knowledge { statement, .. } => statement,
            Self::Procedure { purpose, .. } | Self::Intention { purpose, .. } => purpose,
        };
        if summary.trim().is_empty() {
            return Err("Memory content needs a non-empty statement or purpose".into());
        }
        if let Self::Procedure { method, .. } = self {
            match method {
                ProcedureForm::Advisory {
                    steps,
                    evidence_criteria,
                } if steps.is_empty() || evidence_criteria.is_empty() => {
                    return Err("An advisory procedure needs steps and evidence criteria".into());
                }
                ProcedureForm::Executable { entrypoint, .. } if entrypoint.trim().is_empty() => {
                    return Err("An executable procedure needs an entrypoint".into());
                }
                _ => (),
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MemoryPolicy {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub semantic_triggers: Vec<crate::judgement::JudgementDefinition>,
    pub retention_purpose: String,
    pub allowed_uses: Vec<String>,
    pub source_rules: Vec<String>,
    pub evidence_requirements: Vec<String>,
    pub applicability_rules: Vec<String>,
    pub budget_class: String,
    pub scheduling_priority: i32,
    pub judgement_dispositions: Vec<String>,
    pub notification_policy: String,
    pub qualification_requirements: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consolidation: Option<crate::consolidation::ConsolidationPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PolicyVersion {
    pub reference: ConfigRef,
    pub scope: Scope,
    pub policy: MemoryPolicy,
    pub effective: ValidTime,
    pub recorded: CommitPosition,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PolicyAction {
    Retain,
    Qualify,
    RetrieveFurther,
    Investigate,
    Revise,
    Defer,
    Retire,
    RequestInput,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PolicyDecision {
    pub policy: ConfigRef,
    pub action: PolicyAction,
    pub reason: String,
    pub constraints: Vec<String>,
    pub required_evidence: Vec<String>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RecordDraft {
    pub label: String,
    pub scope: Scope,
    pub content: MemoryContent,
    pub origin: Origin,
    pub evidential_status: EvidentialStatus,
    pub availability: Availability,
    pub qualification: Qualification,
    pub valid_time: ValidTime,
    pub source_locators: Vec<Uuid>,
    pub derived_from: Vec<MemoryRef>,
    pub decision: PolicyDecision,
}

impl RecordDraft {
    pub fn validate(&self) -> Result<(), String> {
        if self.label.trim().is_empty() || self.decision.reason.trim().is_empty() {
            return Err("A record needs a label and policy reason".into());
        }
        self.content.validate()?;
        if let MemoryContent::Intention {
            plan: Some(plan),
            expires_at,
            ..
        } = &self.content
        {
            plan.validate(expires_at.ok_or("Executable intentions need a finite expiry")?)?;
        }
        self.valid_time.validate()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CommitPosition {
    pub sequence: u32,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MemoryVersion {
    pub reference: MemoryRef,
    pub version_id: Uuid,
    pub created_by: Uuid,
    pub recorded: CommitPosition,
    pub recorded_until: Option<u32>,
    pub decision_id: Uuid,
    pub record: RecordDraft,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    Supports,
    Challenges,
    ConflictsWith,
    DerivedFrom,
    DependsOn,
    Supersedes,
    Triggers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Acceptance {
    Candidate,
    Accepted,
    Withdrawn,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RelationDraft {
    pub from: MemoryRef,
    pub to: MemoryRef,
    pub kind: RelationKind,
    pub scope: Scope,
    pub basis: String,
    pub evidential_status: EvidentialStatus,
    pub acceptance: Acceptance,
    pub valid_time: ValidTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RelationVersion {
    pub id: Uuid,
    pub revision: u32,
    pub version_id: Uuid,
    pub recorded: CommitPosition,
    pub recorded_until: Option<u32>,
    pub relation: RelationDraft,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct MemoryQuery {
    pub valid_at: Option<DateTime<Utc>>,
    pub recorded_as_of: Option<u32>,
    pub entity_id: Option<Uuid>,
    pub include_inactive: bool,
    pub after_id: Option<Uuid>,
    pub limit: u16,
}

impl Default for MemoryQuery {
    fn default() -> Self {
        Self {
            valid_at: None,
            recorded_as_of: None,
            entity_id: None,
            include_inactive: false,
            after_id: None,
            limit: 100,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum MemoryChange {
    Create(RecordDraft),
    Revise {
        expected: MemoryRef,
        record: RecordDraft,
    },
}
