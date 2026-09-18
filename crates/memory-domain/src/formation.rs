//! Bounded formation over retained, revisioned tool-event sources.
use crate::{
    contracts::{ConfigRef, Coverage, MemoryRef, Scope, SourceRef, Usage},
    records::{MemoryContent, MemoryVersion},
    sources::{SourceLocator, ToolEvent},
};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Retention {
    Required,
    Optional,
    Temporary,
}
/// Supplied by the source connector or explicit contribution UI, not inferred
/// from text claiming to be a user or an observation.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FormationInput {
    pub episode: String,
    pub objective: String,
    pub conditions: Vec<String>,
    pub boundary_explicit: bool,
    pub retention: Retention,
    pub explicitly_selected: bool,
    pub explicit_contribution: bool,
    pub actor_id: Option<Uuid>,
    pub content: MemoryContent,
    pub evidence: serde_json::Value,
    pub source_locators: Vec<Uuid>,
    pub corrects: Vec<String>,
    #[serde(default)]
    pub based_on: Vec<String>,
    pub uncertainty: Vec<String>,
    pub coverage: Coverage,
}
impl FormationInput {
    pub fn validate(&self) -> Result<(), String> {
        self.content.validate()?;
        if self.episode.trim().is_empty()
            || self.objective.trim().is_empty()
            || (self.explicit_contribution && self.actor_id.is_none())
        {
            return Err(
                "Capture needs episode, objective and an actor for explicit contributions".into(),
            );
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FormationEntry {
    pub position: u32,
    pub duplicate_records: Option<Vec<MemoryRef>>,
    pub previous_deferral: Option<DeferredContribution>,
    pub event: ToolEvent,
    pub input: FormationInput,
    pub locator: SourceLocator,
    pub native_locators: Vec<SourceLocator>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FormationWindow {
    pub id: Uuid,
    pub job_id: Uuid,
    pub source: SourceRef,
    pub operation: String,
    pub policy: ConfigRef,
    pub scope: Scope,
    pub after: u32,
    pub through: u32,
    pub cutoff: DateTime<Utc>,
    pub entries: Vec<FormationEntry>,
    pub remaining: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FormationCommit {
    pub window_id: Uuid,
    /// One selected judgement decision per candidate event.
    pub decisions: BTreeMap<String, Uuid>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeferredContribution {
    pub event_id: String,
    pub reason: String,
    pub required: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SupportGroup {
    pub source: SourceRef,
    pub event_ids: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FormationResult {
    pub window_id: Uuid,
    pub cursor: u32,
    pub records: Vec<MemoryVersion>,
    pub duplicate_events: BTreeMap<String, Vec<MemoryRef>>,
    pub deferred: Vec<DeferredContribution>,
    pub coverage: Coverage,
    pub unresolved: Vec<String>,
    pub inspected: Vec<SourceLocator>,
    pub common_source_groups: Vec<SupportGroup>,
    pub usage: Usage,
    pub judgement_usage: BTreeMap<Uuid, crate::judgement::JudgementUsage>,
}

impl FormationEntry {
    pub fn suppressed(&self) -> bool {
        !self.input.explicitly_selected
            && (self.input.retention == Retention::Temporary
                || matches!(
                    self.event.evidential_status,
                    crate::contracts::EvidentialStatus::Simulation
                        | crate::contracts::EvidentialStatus::Assumption
                ))
    }
    pub fn families(&self) -> Vec<&'static str> {
        let mut families = vec!["J01", "J02"];
        if !self.input.boundary_explicit {
            families.push("J03");
        }
        match &self.input.content {
            MemoryContent::Procedure { .. } => families.push("J04"),
            MemoryContent::Intention { .. } => families.push("J05"),
            _ => {}
        }
        if self.input.explicit_contribution {
            families.push("J27");
        }
        families
    }
    /// All packet evidence points back to the host-admitted window, including the
    /// declared status. No free-form worker summary can replace these fields.
    pub fn evidence_pointer(index: usize, name: &str) -> Option<String> {
        let suffix = match name {
            "candidate" | "claim" | "lesson" | "statement" | "contribution" => "",
            "evidence" | "cases" => "/input/evidence",
            "context" | "actor_context" => "/input",
            "events" => return Some("/entries".into()),
            _ => return None,
        };
        Some(format!("/entries/{index}{suffix}"))
    }
    pub fn record(
        &self,
        window: &FormationWindow,
        decision: crate::records::PolicyDecision,
    ) -> crate::records::RecordDraft {
        use crate::{contracts::EvidentialStatus, records::*};
        let mut content = self.input.content.clone();
        if let MemoryContent::Knowledge {
            uncertainty,
            examined_coverage,
            ..
        } = &mut content
        {
            uncertainty.extend(self.input.uncertainty.clone());
            uncertainty.extend(
                self.input
                    .coverage
                    .unexamined
                    .iter()
                    .map(|s| format!("Not examined: {s}")),
            );
            examined_coverage.extend(self.input.coverage.examined.clone());
        }
        let mut scope = window.scope.clone();
        if self.input.explicit_contribution && matches!(content, MemoryContent::Knowledge { .. }) {
            // A service-wide grant does not make an individual's stated preference
            // a shared rule. Shared adoption requires its own policy decision.
            scope.user_id = self.input.actor_id;
        }
        RecordDraft {
            label: format!("{}: {}", self.input.episode, self.event.event_id),
            scope,
            content,
            origin: self.event.origin,
            evidential_status: self.event.evidential_status,
            availability: if matches!(
                self.event.evidential_status,
                EvidentialStatus::Simulation | EvidentialStatus::Assumption
            ) {
                Availability::Historical
            } else {
                Availability::Routine
            },
            qualification: Qualification::Candidate,
            valid_time: ValidTime::Unknown,
            source_locators: std::iter::once(self.locator.id)
                .chain(self.input.source_locators.iter().copied())
                .collect(),
            derived_from: vec![],
            decision,
        }
    }
}
