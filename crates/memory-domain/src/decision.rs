//! ABOUTME: Contracts for the bounded next-operation decision Jev's Choice answer selects from.
//! ABOUTME: A choice names its operation and, when it acts on existing material, the target the candidate set recorded.
use crate::contracts::MemoryRef;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What a chosen operation would act on, checked against the current store before it executes.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DecisionTarget {
    None,
    Memory { reference: MemoryRef },
    Artifact { artifact_id: Uuid },
}

/// Jev's pick from the bounded candidate set the `decide` tool offered, sent back for the Host's recheck.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DecisionChoice {
    pub operation: String,
    pub target: Option<DecisionTarget>,
}
