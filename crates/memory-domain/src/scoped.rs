use crate::contracts::{ConfigRef, WorkInputs};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DefinitionLookup {
    pub name: String,
    pub artifact_id: Uuid,
    /// RFC 6901 pointer into an assigned JSON artifact.
    pub pointer: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InvestigationStep {
    pub key: String,
    pub question: String,
    pub inputs: WorkInputs,
    pub output_criteria: Vec<String>,
    pub tools: Vec<String>,
    pub reuse_job_id: Option<Uuid>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InvestigationPlan {
    pub method: ConfigRef,
    pub interpretation_conditions: Vec<String>,
    pub definitions: Vec<DefinitionLookup>,
    pub steps: Vec<InvestigationStep>,
}
