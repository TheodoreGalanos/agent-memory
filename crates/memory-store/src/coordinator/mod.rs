//! Host state transitions. Transactions finish before a worker performs I/O.
mod budgets;
mod effects;
pub(crate) mod events;
pub(crate) mod jobs;
pub(crate) mod receipts;
pub(crate) mod scoped;
pub use receipts::MemoryCommit;

use crate::{
    Error, Result,
    access::{read_scope, require_write_scope},
    database::{id, number, timestamp},
};
use memory_domain::{
    contracts::Authority,
    coordination::{Assignment, Fence, Job, JobState},
};
use sqlx::{AnyConnection, Row, any::AnyRow};

pub(crate) async fn job(
    connection: &mut AnyConnection,
    authority: &Authority,
    job_id: uuid::Uuid,
) -> Result<Job> {
    read_scope(connection, authority, job_id).await?;
    let row = sqlx::query("SELECT * FROM jobs WHERE tenant_id=$1 AND id=$2")
        .bind(authority.tenant_id.to_string())
        .bind(job_id.to_string())
        .fetch_optional(connection)
        .await?
        .ok_or(Error::NotFound)?;
    decode_job(&row)
}
fn decode_job(row: &AnyRow) -> Result<Job> {
    Ok(Job {
        id: id(row.try_get("id")?)?,
        state: crate::database::from_tag(row.try_get("state")?)?,
        spec: serde_json::from_str(row.try_get("spec")?)?,
        root_id: id(row.try_get("root_id")?)?,
        depth: number(row.try_get("depth")?)?
            .try_into()
            .map_err(|_| Error::LimitExceeded)?,
        attempt: number(row.try_get("attempt")?)?,
        session_id: row.try_get("session_id")?,
        operation_id: row.try_get("operation_id")?,
        deadline: timestamp(row.try_get("deadline")?)?,
        cancel_requested: row.try_get::<i64, _>("cancel_requested")? != 0,
        wait_reason: row.try_get("wait_reason")?,
        ready_at: row
            .try_get::<Option<i64>, _>("ready_at")?
            .map(timestamp)
            .transpose()?,
        result: row
            .try_get::<Option<String>, _>("result")?
            .map(|s| serde_json::from_str(&s))
            .transpose()?,
    })
}
pub(crate) async fn fence(
    connection: &mut AnyConnection,
    authority: &Authority,
    permit: &Fence,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Job> {
    if permit.owner_id != authority.actor_id {
        return Err(Error::Forbidden);
    }
    require_write_scope(connection, authority, permit.job_id).await?;
    let job = job(connection, authority, permit.job_id).await?;
    let current = sqlx::query("SELECT epoch,owner_id,expires_at,job_id FROM session_leases WHERE tenant_id=$1 AND session_id=$2").bind(authority.tenant_id.to_string()).bind(&job.session_id).fetch_optional(connection).await?.ok_or(Error::Conflict)?;
    if current.try_get::<String, _>("job_id")? != permit.job_id.to_string()
        || current.try_get::<String, _>("owner_id")? != permit.owner_id.to_string()
        || number(current.try_get("epoch")?)? != permit.epoch
        || current.try_get::<i64, _>("expires_at")? <= now.timestamp_millis()
        || !matches!(job.state, JobState::Leased | JobState::Running)
    {
        return Err(Error::Conflict);
    }
    Ok(job)
}
fn assignment(
    job: Job,
    owner_id: uuid::Uuid,
    epoch: u32,
    expires_at: chrono::DateTime<chrono::Utc>,
) -> Assignment {
    Assignment {
        job,
        owner_id,
        epoch,
        expires_at,
    }
}
