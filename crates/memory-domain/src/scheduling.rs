//! ABOUTME: Wire type for inspecting the durable scheduling ledger the Host timer writes to when it
//! ABOUTME: turns consolidation, qualification, maintenance and intention triggers into submitted jobs.
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One row of the `scheduled_work` ledger: what a sweep decided about a single trigger, so a
/// repeated sweep is idempotent and an operator can see why something was, or was not, scheduled.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScheduledWorkMark {
    pub trigger_kind: String,
    pub trigger_id: Uuid,
    pub dedupe_key: String,
    pub job_id: Option<Uuid>,
    pub unschedulable: Option<String>,
    pub causal_origin_job_id: Option<Uuid>,
    pub scheduled_at: DateTime<Utc>,
}
