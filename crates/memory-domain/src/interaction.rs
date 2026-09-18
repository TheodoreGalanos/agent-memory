//! User and operator operations share the Host's authentication and scoped records.
use crate::{contracts::*, coordination::*, records::*, workspace::RenderManifest};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum UserRequest {
    Identity,
    Browse {
        query: MemoryQuery,
    },
    History {
        memory_id: Uuid,
        after_revision: u32,
        limit: u16,
    },
    InspectMemory {
        reference: MemoryRef,
    },
    InspectTask {
        job_id: Uuid,
        after_manifest: Option<Uuid>,
    },
    InspectExploration {
        id: Uuid,
    },
    Decisions {
        job_id: Uuid,
    },
    Changes {
        after: u32,
        limit: u16,
    },
    Notifications {
        after: u32,
        limit: u16,
    },
    Policy {
        reference: ConfigRef,
    },
    Controls,
    Mutate {
        request_id: Uuid,
        mutation: Box<UserMutation>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum UserMutation {
    Contribute {
        record: Box<RecordDraft>,
    },
    Correct {
        expected: MemoryRef,
        record: Box<RecordDraft>,
    },
    OpenExploration {
        scope: Scope,
        purpose: String,
        expires_at: DateTime<Utc>,
    },
    AddExploration {
        id: Uuid,
        expected_revision: u32,
        record: Box<RecordDraft>,
    },
    PromoteExploration {
        id: Uuid,
        expected_revision: u32,
        index: u16,
    },
    RequestDecision {
        job_id: Uuid,
        #[serde(default)]
        fence: Option<Fence>,
        owner_id: Uuid,
        question: String,
        missing: String,
        deadline: DateTime<Utc>,
    },
    AnswerDecision {
        id: Uuid,
        expected_revision: u32,
        answer: DecisionAnswer,
        reason: String,
    },
    NotificationPreference {
        mode: NotificationMode,
    },
    AcknowledgeNotification {
        event_id: Uuid,
    },
    SetControl {
        kind: ControlKind,
        target: String,
        expected_revision: u32,
        enabled: bool,
    },
    CreatePolicy {
        label: String,
        scope: Scope,
        policy: Box<MemoryPolicy>,
        effective: ValidTime,
    },
    RevisePolicy {
        expected: ConfigRef,
        policy: Box<MemoryPolicy>,
        effective: ValidTime,
    },
}
impl UserMutation {
    pub fn administrator_only(&self) -> bool {
        matches!(
            self,
            Self::SetControl { .. } | Self::CreatePolicy { .. } | Self::RevisePolicy { .. }
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UserResponse {
    Identity {
        tenant_id: Uuid,
        actor_id: Uuid,
        scope: Scope,
    },
    Records {
        records: Vec<MemoryVersion>,
        next: Option<Uuid>,
    },
    History {
        records: Vec<MemoryVersion>,
        next_revision: Option<u32>,
    },
    Memory {
        memory: Box<MemoryVersion>,
        relations: Vec<RelationVersion>,
    },
    Task {
        job: Box<Job>,
        manifests: Vec<RenderManifest>,
        next_manifest: Option<Uuid>,
        decisions: Vec<DecisionRequest>,
        coverage: Coverage,
    },
    Exploration {
        exploration: Box<Exploration>,
    },
    Decisions {
        decisions: Vec<DecisionRequest>,
    },
    Changes {
        page: EventPage,
        revisions: Vec<crate::maintenance::MemoryChangeNotice>,
    },
    Notifications {
        page: EventPage,
        mode: NotificationMode,
    },
    Policy {
        policy: Box<PolicyVersion>,
    },
    Controls {
        controls: Vec<RuntimeControl>,
    },
    Done,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Exploration {
    pub id: Uuid,
    pub revision: u32,
    pub scope: Scope,
    pub purpose: String,
    pub expires_at: DateTime<Utc>,
    pub records: Vec<RecordDraft>,
    pub promoted: Vec<u16>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DecisionAnswer {
    Approve,
    Decline,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DecisionRequest {
    pub id: Uuid,
    pub revision: u32,
    pub job_id: Uuid,
    pub scope: Scope,
    pub owner_id: Uuid,
    pub question: String,
    pub missing: String,
    pub deadline: DateTime<Utc>,
    pub answer: Option<DecisionAnswer>,
    pub reason: Option<String>,
    pub answered_by: Option<Uuid>,
    /// Unanswered or declined requests keep affected execution blocked, including after expiry.
    pub fallback: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotificationMode {
    Material,
    Blockers,
    Completion,
    Muted,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ControlKind {
    Provider,
    Family,
    Profile,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RuntimeControl {
    pub kind: ControlKind,
    pub target: String,
    pub revision: u32,
    pub enabled: bool,
}
