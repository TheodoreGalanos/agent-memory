//! Intention definitions live in memory records; occurrences retain their execution snapshot.
use crate::{contracts::*, coordination::Event, judgement::JudgementDefinition, records::*};
use chrono::{DateTime, Duration, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IntentionTrigger {
    Time {
        at: DateTime<Utc>,
    },
    Event {
        resource_id: Uuid,
        event_kind: String,
    },
    SourceRevision {
        source_id: Uuid,
        after_revision: String,
    },
    Result {
        job_id: Uuid,
    },
    Semantic {
        definition: Box<JudgementDefinition>,
        matched_choice: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CompletionRule {
    pub result_pointer: String,
    pub equals: serde_json::Value,
    /// Invocation IDs within the execution job. The Host supplies the operation prefix.
    pub required_effects: Vec<String>,
    pub semantic_conditions: Vec<String>,
    pub confirmation_owner: Option<Uuid>,
    /// When set, a host-recorded job completion before expiry can be delivered within this grace.
    pub delivery_grace_seconds: Option<u32>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IntentionPlan {
    pub trigger: IntentionTrigger,
    pub execution: Box<WorkBrief>,
    pub ready_memories: Vec<MemoryRef>,
    pub ready_artifacts: Vec<Uuid>,
    pub completion: CompletionRule,
    /// Fixed intervals only; missed windows expire without a backlog of executions.
    pub recurrence_seconds: Option<u32>,
}
impl IntentionPlan {
    pub fn validate(&self, expires: DateTime<Utc>) -> Result<(), String> {
        if self.recurrence_seconds.is_some_and(|s| s == 0)
            || self
                .completion
                .delivery_grace_seconds
                .is_some_and(|s| s > 86400)
            || self.ready_memories.len() > 32
            || self.ready_artifacts.len() > 32
            || self.completion.semantic_conditions.len() > 16
            || self.completion.required_effects.len() > 32
            || (!self.completion.result_pointer.is_empty()
                && !self.completion.result_pointer.starts_with('/'))
        {
            return Err("Invalid intention bounds or completion pointer".into());
        }
        if let IntentionTrigger::Time { at } = &self.trigger {
            if *at >= expires {
                return Err("Trigger must precede the definition expiry".into());
            }
        }
        if self.recurrence_seconds.is_some()
            && !matches!(self.trigger, IntentionTrigger::Time { .. })
        {
            return Err("Recurrence requires a time trigger".into());
        }
        if let IntentionTrigger::Semantic {
            definition,
            matched_choice,
        } = &self.trigger
        {
            definition.validate()?;
            if definition.questions.len() != 1
                || definition.input_requirements != ["intention", "event"]
                || !definition
                    .permitted_uses
                    .iter()
                    .any(|s| s == "candidate_intentions")
                || !matches!(&definition.questions[0].form,crate::judgement::QuestionForm::Choice{criteria} if criteria.contains_key(matched_choice))
            {
                return Err("Semantic trigger needs one established choice question over intention and event".into());
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IntentionState {
    Pending,
    Armed,
    Fired,
    Completed,
    Expired,
    Cancelled,
}
impl IntentionState {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Expired | Self::Cancelled)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IntentionOccurrence {
    pub id: Uuid,
    pub definition: MemoryVersion,
    pub cycle: u32,
    pub previous: Option<Uuid>,
    pub state: IntentionState,
    pub event_cursor: u32,
    pub due_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub job_id: Option<Uuid>,
    pub trigger_event: Option<Uuid>,
    pub confirmed_by: Option<Uuid>,
    pub reason: Option<String>,
    pub evidence: Vec<Uuid>,
    pub unresolved: Vec<String>,
}
impl IntentionOccurrence {
    pub fn plan(&self) -> Option<&IntentionPlan> {
        match &self.definition.record.content {
            MemoryContent::Intention { plan, .. } => plan.as_deref(),
            _ => None,
        }
    }
    pub fn terminal_due(&self) -> Option<DateTime<Utc>> {
        self.expires_at.map(|at| {
            at + Duration::seconds(i64::from(
                self.plan()
                    .filter(|_| self.state == IntentionState::Fired)
                    .and_then(|p| p.completion.delivery_grace_seconds)
                    .unwrap_or(0),
            ))
        })
    }
    pub fn accepts_completion(&self, now: DateTime<Utc>, host_completed_at: DateTime<Utc>) -> bool {
        self.state == IntentionState::Fired
            && self.terminal_due().is_some_and(|at| now < at)
            && if self
                .plan()
                .is_some_and(|p| p.completion.delivery_grace_seconds.is_some())
            {
                self.expires_at.is_some_and(|at| host_completed_at < at)
            } else {
                self.expires_at.is_some_and(|at| now < at)
            }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IntentionCheckKind {
    Readiness,
    Trigger { event_id: Uuid },
    Completion,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IntentionCheck {
    pub id: Uuid,
    pub occurrence: IntentionOccurrence,
    pub kind: IntentionCheckKind,
    pub event: Option<Event>,
    pub evidence: serde_json::Value,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IntentionSweep {
    pub occurrences: Vec<IntentionOccurrence>,
    pub more: bool,
}
