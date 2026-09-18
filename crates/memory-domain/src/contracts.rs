//! Wire contracts for the first implementation increment. Persistence and effects
//! are deliberately left to the packages that implement them.
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum WireVersion {
    #[serde(rename = "1")]
    V1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MemoryRef {
    pub memory_id: Uuid,
    #[schemars(range(max = 4294967295_u32))]
    pub revision: NonZeroU32,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ConfigRef {
    pub id: Uuid,
    #[schemars(range(max = 4294967295_u32))]
    pub revision: NonZeroU32,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SourceRef {
    pub source_id: Uuid,
    pub revision: String,
}

/// Absent dimensions are unrestricted. An assigned restriction cannot be removed
/// by leaving that dimension out of a request. Tenant is checked separately.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Scope {
    pub user_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
    pub entity_ids: Vec<Uuid>,
    pub source_versions: Vec<SourceRef>,
}

impl Scope {
    pub fn permits(&self, requested: &Self) -> bool {
        fn dimension(grant: Option<Uuid>, request: Option<Uuid>) -> bool {
            grant.is_none() || grant == request
        }
        fn subset<T: PartialEq>(grant: &[T], request: &[T]) -> bool {
            grant.is_empty() || (!request.is_empty() && request.iter().all(|x| grant.contains(x)))
        }
        dimension(self.user_id, requested.user_id)
            && dimension(self.project_id, requested.project_id)
            && dimension(self.task_id, requested.task_id)
            && subset(&self.entity_ids, &requested.entity_ids)
            && subset(&self.source_versions, &requested.source_versions)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Process {
    Formation,
    Activation,
    Consolidation,
    Maintenance,
    Investigation,
    Evaluation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Observed,
    Activated,
    AgentGenerated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EvidentialStatus {
    Observation,
    AttributedStatement,
    Inference,
    Assumption,
    Simulation,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkInputs {
    pub sources: Vec<SourceRef>,
    pub memories: Vec<MemoryRef>,
    pub artifacts: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Capabilities {
    pub sources: Vec<SourceRef>,
    pub queries: Vec<String>,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkLimits {
    pub root_budget_id: Uuid,
    #[schemars(range(max = 4294967295_u32))]
    pub max_provider_attempts: NonZeroU32,
    #[schemars(range(max = 4294967295_u32))]
    pub max_tokens: NonZeroU32,
    #[schemars(range(max = 4294967295_u32))]
    pub max_output_bytes: NonZeroU32,
    pub max_child_depth: u16,
    pub max_child_concurrency: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkBrief {
    pub schema_version: WireVersion,
    pub purpose: String,
    pub process: Process,
    pub task_id: Uuid,
    pub frame_ref: Option<Uuid>,
    pub definitions: Vec<String>,
    pub scope: Scope,
    pub inputs: WorkInputs,
    pub capabilities: Capabilities,
    pub known_conflicts: Vec<String>,
    pub evidence_cutoff: DateTime<Utc>,
    pub freshness_requirement: String,
    pub output_criteria: Vec<String>,
    pub limits: WorkLimits,
    pub policy: ConfigRef,
    pub profile: ConfigRef,
    pub retention_policy: String,
    pub disclosure_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Command<T> {
    pub request_id: Uuid,
    pub command_version: WireVersion,
    pub tenant_id: Uuid,
    pub actor_id: Uuid,
    pub scope: Scope,
    pub job_id: Option<Uuid>,
    // Pi identifiers are opaque strings, not domain UUIDs.
    pub session_id: Option<String>,
    pub lane_id: Option<String>,
    pub operation_id: Option<String>,
    pub invocation_id: Option<String>,
    pub expected_revisions: Vec<MemoryRef>,
    pub deadline: DateTime<Utc>,
    pub budget_id: Uuid,
    #[schemars(range(max = 4294967295_u32))]
    pub lease_epoch: Option<NonZeroU32>,
    pub payload: T,
}

/// Constructed by authentication/assignment code, never from the command body.
#[derive(Debug, Clone)]
pub struct Authority {
    pub tenant_id: Uuid,
    pub actor_id: Uuid,
    pub scope: Scope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    InvalidPayload,
    Unauthenticated,
    ForbiddenScope,
    StaleRevision,
    RequestConflict,
    DeadlineExceeded,
    BudgetExhausted,
    InsufficientEvidence,
    SourceUnavailable,
    ProviderUnavailable,
    Cancelled,
    UnknownEffect,
    InternalError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContractError {
    pub code: ReasonCode,
    pub message: String,
}

fn invalid(message: &str) -> ContractError {
    ContractError {
        code: ReasonCode::InvalidPayload,
        message: message.into(),
    }
}

impl<T> Command<T> {
    pub fn check_authority(
        &self,
        authority: &Authority,
        now: DateTime<Utc>,
    ) -> Result<(), ContractError> {
        if self.tenant_id != authority.tenant_id
            || self.actor_id != authority.actor_id
            || !authority.scope.permits(&self.scope)
        {
            return Err(ContractError {
                code: ReasonCode::ForbiddenScope,
                message: "Command exceeds its authenticated assignment".into(),
            });
        }
        if self.deadline <= now {
            return Err(ContractError {
                code: ReasonCode::DeadlineExceeded,
                message: "Command deadline has passed".into(),
            });
        }
        Ok(())
    }
}

impl Command<WorkBrief> {
    pub fn validate(&self) -> Result<(), ContractError> {
        let brief = &self.payload;
        if brief.purpose.trim().is_empty() || brief.output_criteria.is_empty() {
            return Err(invalid("Work needs a purpose and output criteria"));
        }
        if !self.scope.permits(&brief.scope)
            || brief.scope.task_id != Some(brief.task_id)
            || self.budget_id != brief.limits.root_budget_id
        {
            return Err(invalid(
                "Brief scope, task or budget differs from its command",
            ));
        }
        for source in brief
            .inputs
            .sources
            .iter()
            .chain(&brief.capabilities.sources)
        {
            if source.revision.trim().is_empty()
                || (!brief.scope.source_versions.is_empty()
                    && !brief.scope.source_versions.contains(source))
            {
                return Err(invalid(
                    "Brief includes a source outside its declared revisions",
                ));
            }
        }
        if brief.evidence_cutoff > self.deadline {
            return Err(invalid(
                "Evidence cutoff is later than the command deadline",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkStatus {
    Complete,
    Partial,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Finding {
    pub statement: String,
    pub origin: Origin,
    pub evidential_status: EvidentialStatus,
    pub sources: Vec<SourceRef>,
    pub supporting_memories: Vec<MemoryRef>,
    pub challenging_memories: Vec<MemoryRef>,
    pub applicability: Scope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EffectStatus {
    Confirmed,
    NotPerformed,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct KnownEffect {
    pub effect_id: Uuid,
    pub status: EffectStatus,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UsageStatus {
    Known,
    Partial,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Money {
    pub currency: String,
    pub minor_units: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Usage {
    pub status: UsageStatus,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
    pub cost: Option<Money>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Coverage {
    pub examined: Vec<String>,
    pub unexamined: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkResult {
    pub schema_version: WireVersion,
    pub status: WorkStatus,
    pub examined_scope: Scope,
    pub inputs: WorkInputs,
    pub findings: Vec<Finding>,
    pub coverage: Coverage,
    pub unresolved_work: Vec<String>,
    pub proposed_changes: Vec<Uuid>,
    pub child_outputs: Vec<Uuid>,
    pub result_artifact: Option<Uuid>,
    pub known_effects: Vec<KnownEffect>,
    pub usage: Usage,
}

impl WorkResult {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.status != WorkStatus::Complete && self.unresolved_work.is_empty() {
            return Err(invalid(
                "Partial or blocked work must explain what remains unresolved",
            ));
        }
        if self.status == WorkStatus::Complete
            && (!self.unresolved_work.is_empty()
                || !self.coverage.unexamined.is_empty()
                || self
                    .known_effects
                    .iter()
                    .any(|effect| effect.status == EffectStatus::Unknown))
        {
            return Err(invalid(
                "Unresolved work, coverage or effects cannot be reported complete",
            ));
        }
        if self.inputs.sources.iter().any(|source| {
            source.revision.trim().is_empty()
                || (!self.examined_scope.source_versions.is_empty()
                    && !self.examined_scope.source_versions.contains(source))
        }) {
            return Err(invalid(
                "Result inputs exceed the examined source revisions",
            ));
        }
        for finding in &self.findings {
            if finding.statement.trim().is_empty()
                || !self.examined_scope.permits(&finding.applicability)
                || finding
                    .sources
                    .iter()
                    .any(|source| !self.inputs.sources.contains(source))
                || finding
                    .supporting_memories
                    .iter()
                    .chain(&finding.challenging_memories)
                    .any(|reference| {
                        !self.inputs.memories.iter().any(|input| {
                            input.memory_id == reference.memory_id
                                && input.revision == reference.revision
                        })
                    })
            {
                return Err(invalid("Finding exceeds the scope or sources examined"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStatus {
    Accepted,
    Completed,
    Partial,
    Blocked,
    Conflict,
    Unavailable,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CommandResponse {
    pub schema_version: WireVersion,
    pub request_id: Uuid,
    pub status: ResponseStatus,
    pub reason: Option<ContractError>,
    pub job_id: Option<Uuid>,
    pub result_ref: Option<Uuid>,
    pub known_effects: Vec<KnownEffect>,
    pub coverage: Coverage,
    pub usage: Usage,
}
