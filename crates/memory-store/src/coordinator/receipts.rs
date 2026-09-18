use super::{events::append_event, fence};
use crate::{Error, Result, Store, access::require_write_scope};
use memory_domain::{
    contracts::{Authority, Command},
    records::{MemoryChange, MemoryVersion},
};
use serde::{Serialize, de::DeserializeOwned};
use sqlx::{AnyConnection, Row};
use uuid::Uuid;

pub use memory_domain::coordination::MemoryCommit;

pub(crate) async fn existing<T: DeserializeOwned>(
    connection: &mut AnyConnection,
    authority: &Authority,
    request_id: Uuid,
    kind: &str,
    request: &impl Serialize,
) -> Result<Option<T>> {
    let row = sqlx::query("SELECT resource_id,kind,request,result FROM command_receipts WHERE tenant_id=$1 AND request_id=$2").bind(authority.tenant_id.to_string()).bind(request_id.to_string()).fetch_optional(&mut *connection).await?;
    let Some(row) = row else {
        return Ok(None);
    };
    crate::retention::check_resource(
        connection,
        authority,
        crate::database::id(row.try_get("resource_id")?)?,
    )
    .await?;
    require_write_scope(
        connection,
        authority,
        crate::database::id(row.try_get("resource_id")?)?,
    )
    .await?;
    let request_data: Option<String> = row.try_get("request")?;
    let Some(request_data) = request_data else {
        return Err(Error::Unavailable);
    };
    let prior: serde_json::Value = serde_json::from_str(&request_data)?;
    if row.try_get::<String, _>("kind")? != kind || prior != serde_json::to_value(request)? {
        return Err(Error::Conflict);
    }
    Ok(Some(serde_json::from_str(row.try_get("result")?)?))
}
pub(crate) async fn save(
    connection: &mut AnyConnection,
    authority: &Authority,
    request_id: Uuid,
    resource_id: Uuid,
    kind: &str,
    request: &impl Serialize,
    result: &impl Serialize,
) -> Result<()> {
    sqlx::query("INSERT INTO command_receipts (tenant_id,request_id,actor_id,resource_id,kind,request,result) VALUES ($1,$2,$3,$4,$5,$6,$7)")
        .bind(authority.tenant_id.to_string()).bind(request_id.to_string()).bind(authority.actor_id.to_string()).bind(resource_id.to_string()).bind(kind).bind(serde_json::to_string(request)?).bind(serde_json::to_string(result)?).execute(connection).await?;
    Ok(())
}
impl Store {
    pub async fn commit_memories(
        &self,
        authority: &Authority,
        command: Command<MemoryCommit>,
    ) -> Result<Vec<MemoryVersion>> {
        let (mut tx, position) = self.begin_write().await?;
        // Authentication is still required to recover an earlier receipt after its deadline.
        check_identity(authority, &command)?;
        if command.job_id != Some(command.payload.fence.job_id) {
            return Err(Error::Forbidden);
        }
        // Ownership changes between attempts; the logical mutation does not.
        let mut request = serde_json::to_value(&command)?;
        let fields = request.as_object_mut().expect("command is an object");
        fields.remove("actor_id");
        fields.remove("lease_epoch");
        fields.remove("deadline");
        request["payload"]
            .as_object_mut()
            .expect("commit is an object")
            .remove("fence");
        if let Some(result) = existing(
            &mut tx,
            authority,
            command.request_id,
            "memory_commit",
            &request,
        )
        .await?
        {
            return Ok(result);
        }
        command
            .check_authority(authority, position.recorded_at)
            .map_err(|e| Error::Invalid(e.message))?;
        let job = fence(
            &mut tx,
            authority,
            &command.payload.fence,
            position.recorded_at,
        )
        .await?;
        if job.cancel_requested
            || job.deadline <= position.recorded_at
            || command.job_id != Some(job.id)
            || command.budget_id != job.spec.brief.limits.root_budget_id
            || command.session_id.as_deref() != Some(job.session_id.as_str())
            || command.operation_id.as_deref() != Some(job.operation_id.as_str())
            || command.lease_epoch.map(|e| e.get()) != Some(command.payload.fence.epoch)
        {
            return Err(Error::Conflict);
        }
        let assigned = Authority {
            tenant_id: authority.tenant_id,
            actor_id: authority.actor_id,
            scope: job.spec.brief.scope.clone(),
        };
        if !assigned.scope.permits(&command.scope) {
            return Err(Error::Forbidden);
        }
        let revisions: Vec<_> = command
            .payload
            .changes
            .iter()
            .filter_map(|change| match change {
                MemoryChange::Revise { expected, .. } => Some(expected.clone()),
                _ => None,
            })
            .collect();
        if command.expected_revisions != revisions {
            return Err(Error::Invalid(
                "Expected revisions must describe the submitted corrections".into(),
            ));
        }
        let narrow = Authority {
            scope: command.scope.clone(),
            ..assigned
        };
        for change in &command.payload.changes {
            let record = match change {
                MemoryChange::Create(r) | MemoryChange::Revise { record: r, .. } => r,
            };
            crate::intentions::check_plan(&mut tx, &narrow, &job, record).await?;
        }
        let versions = crate::memories::apply_changes(
            &mut tx,
            &narrow,
            command.payload.changes.clone(),
            position,
        )
        .await?;
        save(
            &mut tx,
            authority,
            command.request_id,
            job.id,
            "memory_commit",
            &request,
            &versions,
        )
        .await?;
        append_event(
            &mut tx,
            authority,
            job.id,
            "memory_changed",
            position,
            job.spec.retain_until,
        )
        .await?;
        tx.commit().await?;
        Ok(versions)
    }
}
pub(crate) fn check_identity<T>(authority: &Authority, command: &Command<T>) -> Result<()> {
    if authority.tenant_id != command.tenant_id
        || authority.actor_id != command.actor_id
        || !authority.scope.permits(&command.scope)
    {
        return Err(Error::Forbidden);
    }
    Ok(())
}
