use crate::contracts::{EvidentialStatus, Origin, Scope, SourceRef};
use crate::records::Acceptance;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Entity {
    pub id: Uuid,
    pub scope: Scope,
    pub provider: String,
    pub native_id: String,
    pub label: String,
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EntityLink {
    pub id: Uuid,
    pub revision: u32,
    pub from: Uuid,
    pub to: Uuid,
    pub acceptance: Acceptance,
    pub basis: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Document,
    Table,
    Model,
    ToolEvents,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SourceVersion {
    pub reference: SourceRef,
    pub label: String,
    pub scope: Scope,
    pub kind: SourceKind,
    pub owner: String,
    pub acquired_at: DateTime<Utc>,
    pub acquisition_method: String,
    pub precedence: Option<String>,
    pub snapshot_artifact: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Locator {
    Document {
        start_line: u32,
        end_line: u32,
        page: Option<u32>,
    },
    Table {
        row: u32,
        column: Option<String>,
    },
    Model {
        entity: String,
        property: String,
    },
    Image {
        region: [u32; 4],
    },
    Events {
        start: u32,
        end: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SourceLocator {
    pub id: Uuid,
    pub source: SourceRef,
    pub locator: Locator,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolEvent {
    pub event_id: String,
    pub observed_at: DateTime<Utc>,
    pub kind: String,
    pub origin: Origin,
    pub evidential_status: EvidentialStatus,
    pub content: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ArtifactSpec {
    pub label: String,
    pub scope: Scope,
    pub media_type: String,
    pub expected_bytes: u32,
    pub origin: Origin,
    pub retention_class: String,
    pub dependencies: Vec<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactState {
    Pending,
    Uploading,
    Ready,
    Revoked,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Artifact {
    pub id: Uuid,
    pub revision: u32,
    pub state: ArtifactState,
    pub spec: ArtifactSpec,
}
