//! ABOUTME: The durable link from a settled task job to the experience source its worker published, which
//! ABOUTME: later schedules formation over ordinary task evidence without any caller authoring candidates.
use crate::{
    Error, Result, Store,
    access::read_scope,
    coordinator::{events::append_event, fence},
};
use chrono::{Duration, Utc};
use memory_domain::{
    contracts::{Authority, Capabilities, Command, Process, SourceRef, WireVersion, WorkInputs},
    coordination::{Fence, SubmitJob},
};
use sqlx::Row;
use uuid::Uuid;

/// A scheduled formation job gets this long from scheduling, inside the task's budget deadline.
const FORMATION_WINDOW: Duration = Duration::minutes(30);
/// Marks an experience the scheduler examined and could not schedule; it is not retried.
const UNSCHEDULABLE: &str = "unschedulable";

/// Deterministic identities derived from the task job so a repeated sweep submits the same command.
fn derived(job: Uuid, salt: u128) -> Uuid {
    let mixed = job.as_u128() ^ salt;
    // Keep RFC 4122 version (4) and variant bits so the value stays a well-formed UUID.
    Uuid::from_u128(
        (mixed & !0xF000_0000_0000_C000_0000_0000_0000) | 0x4000_0000_0000_8000_0000_0000_0000,
    )
}

impl Store {
    /// Records the experience source of the fenced job's current attempt and appends the durable event.
    pub async fn record_experience(
        &self,
        authority: &Authority,
        permit: &Fence,
        source: &SourceRef,
    ) -> Result<()> {
        let (mut tx, position) = self.begin_write().await?;
        let job = fence(&mut tx, authority, permit, position.recorded_at).await?;
        sqlx::query(
            "INSERT INTO job_experiences (tenant_id,job_id,source_id,revision,published_at) VALUES ($1,$2,$3,$4,$5) \
             ON CONFLICT (tenant_id,job_id) DO UPDATE SET source_id=excluded.source_id,revision=excluded.revision,published_at=excluded.published_at,formation_job_id=NULL",
        )
        .bind(authority.tenant_id.to_string())
        .bind(job.id.to_string())
        .bind(source.source_id.to_string())
        .bind(&source.revision)
        .bind(position.recorded_at.timestamp_millis())
        .execute(&mut *tx)
        .await?;
        append_event(
            &mut tx,
            authority,
            job.id,
            "experience_published",
            position,
            job.spec.retain_until,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Schedules formation for settled tasks whose experience is published and not yet formed. Each
    /// formation job runs under the task's policy and root budget with the experience as its only
    /// source, keyed deterministically so a repeated sweep submits nothing twice. Returns how many were
    /// scheduled; experiences that cannot be scheduled (budget expired, invalid brief) are marked and
    /// left alone.
    pub async fn schedule_formation(&self, authority: &Authority, limit: u16) -> Result<u32> {
        let candidates = sqlx::query(
            "SELECT e.job_id,e.source_id,e.revision FROM job_experiences e JOIN jobs j ON j.tenant_id=e.tenant_id AND j.id=e.job_id \
             WHERE e.tenant_id=$1 AND e.formation_job_id IS NULL AND j.state IN ('completed','partial') ORDER BY e.published_at LIMIT $2",
        )
        .bind(authority.tenant_id.to_string())
        .bind(i64::from(limit))
        .fetch_all(&self.pool)
        .await?;
        let mut scheduled = 0;
        for row in candidates {
            let task_job: Uuid = crate::database::id(row.try_get("job_id")?)?;
            let source = SourceRef {
                source_id: crate::database::id(row.try_get("source_id")?)?,
                revision: row.try_get("revision")?,
            };
            {
                let mut connection = self.pool.acquire().await?;
                if read_scope(&mut connection, authority, task_job)
                    .await
                    .is_err()
                {
                    continue; // Another authority's scope; its own timer schedules it.
                }
            }
            let outcome = self.schedule_one(authority, task_job, &source).await;
            let mark = match outcome {
                Ok(id) => {
                    scheduled += 1;
                    id.to_string()
                }
                Err(Error::Invalid(reason)) => format!("{UNSCHEDULABLE}: {reason}"),
                Err(Error::Forbidden) | Err(Error::LimitExceeded) => UNSCHEDULABLE.to_string(),
                Err(other) => return Err(other),
            };
            sqlx::query("UPDATE job_experiences SET formation_job_id=$1 WHERE tenant_id=$2 AND job_id=$3 AND formation_job_id IS NULL")
                .bind(mark)
                .bind(authority.tenant_id.to_string())
                .bind(task_job.to_string())
                .execute(&self.pool)
                .await?;
        }
        Ok(scheduled)
    }

    async fn schedule_one(
        &self,
        authority: &Authority,
        task_job: Uuid,
        source: &SourceRef,
    ) -> Result<Uuid> {
        let task = self.job(authority, task_job).await?;
        let budget = crate::coordinator::budgets::budget(
            &mut *self.pool.acquire().await?,
            authority,
            task.spec.brief.limits.root_budget_id,
        )
        .await?;
        let now = Utc::now();
        let deadline = (now + FORMATION_WINDOW).min(budget.deadline);
        if deadline <= now + Duration::seconds(30) {
            return Err(Error::Invalid(
                "Budget deadline leaves no time for formation".into(),
            ));
        }
        let mut brief = task.spec.brief.clone();
        let formation_task = derived(task_job, 0x6f72_6d61_7469_6f6e_2d74_6173_6b00_0001);
        brief.process = Process::Formation;
        brief.purpose = format!("form experience of task {}", task.spec.brief.task_id);
        brief.task_id = formation_task;
        brief.scope.task_id = Some(formation_task);
        brief.inputs = WorkInputs {
            sources: vec![source.clone()],
            memories: vec![],
            artifacts: vec![],
        };
        brief.capabilities = Capabilities {
            sources: vec![source.clone()],
            queries: vec![],
            tools: vec![],
        };
        brief.known_conflicts = vec![];
        brief.evidence_cutoff = now;
        let scope = brief.scope.clone();
        let job = self
            .submit_job(
                authority,
                Command {
                    request_id: derived(task_job, 0x666f_726d_6174_696f_6e2d_7265_7100_0002),
                    command_version: WireVersion::V1,
                    tenant_id: authority.tenant_id,
                    actor_id: authority.actor_id,
                    scope,
                    job_id: None,
                    session_id: None,
                    operation_id: None,
                    lane_id: None,
                    invocation_id: None,
                    expected_revisions: vec![],
                    deadline,
                    budget_id: task.spec.brief.limits.root_budget_id,
                    lease_epoch: None,
                    payload: SubmitJob {
                        brief,
                        parent_id: None,
                        max_attempts: 2,
                        retain_until: task.spec.retain_until.max(deadline + Duration::days(1)),
                    },
                },
            )
            .await?;
        Ok(job.id)
    }

    /// What the scheduler recorded for a task's experience: the formation job id, an unschedulable
    /// mark with its reason, or nothing yet.
    pub async fn scheduled_formation(
        &self,
        authority: &Authority,
        job_id: Uuid,
    ) -> Result<Option<String>> {
        let mut connection = self.pool.acquire().await?;
        read_scope(&mut connection, authority, job_id).await?;
        let row = sqlx::query(
            "SELECT formation_job_id FROM job_experiences WHERE tenant_id=$1 AND job_id=$2",
        )
        .bind(authority.tenant_id.to_string())
        .bind(job_id.to_string())
        .fetch_optional(&mut *connection)
        .await?;
        Ok(row.and_then(|row| {
            row.try_get::<Option<String>, _>("formation_job_id")
                .ok()
                .flatten()
        }))
    }

    /// The experience source published for a job, if any.
    pub async fn experience(
        &self,
        authority: &Authority,
        job_id: Uuid,
    ) -> Result<Option<SourceRef>> {
        let mut connection = self.pool.acquire().await?;
        read_scope(&mut connection, authority, job_id).await?;
        let row = sqlx::query(
            "SELECT source_id,revision FROM job_experiences WHERE tenant_id=$1 AND job_id=$2",
        )
        .bind(authority.tenant_id.to_string())
        .bind(job_id.to_string())
        .fetch_optional(&mut *connection)
        .await?;
        row.map(|row| {
            Ok(SourceRef {
                source_id: crate::database::id(row.try_get("source_id")?)?,
                revision: row.try_get("revision")?,
            })
        })
        .transpose()
        .map_err(|e: Error| e)
    }
}
