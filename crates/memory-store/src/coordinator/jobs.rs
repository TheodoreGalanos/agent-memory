use super::{
    assignment,
    budgets::budget,
    events::append_event,
    fence, job,
    receipts::{check_identity, existing, save},
};
use crate::{
    Error, Result, Store,
    access::{predicate, require_write_scope, write_scope},
    database::{number, tag},
};
use chrono::{DateTime, Duration, Utc};
use memory_domain::{
    contracts::{Authority, Command, Process, WorkResult, WorkStatus},
    coordination::{Assignment, Fence, Job, JobState, SubmitJob},
};
use sqlx::{AnyConnection, Row};
use uuid::Uuid;

impl Store {
    pub async fn assigned_job(&self, authority: &Authority, permit: &Fence) -> Result<Job> {
        let (mut tx, position) = self.begin_write().await?;
        fence(&mut tx, authority, permit, position.recorded_at).await
    }

    pub async fn inspect_assignment(
        &self,
        authority: &Authority,
        permit: &Fence,
    ) -> Result<Assignment> {
        let (mut tx, position) = self.begin_write().await?;
        let current = fence(&mut tx, authority, permit, position.recorded_at).await?;
        let row = sqlx::query(
            "SELECT expires_at FROM session_leases WHERE tenant_id=$1 AND session_id=$2",
        )
        .bind(authority.tenant_id.to_string())
        .bind(&current.session_id)
        .fetch_one(&mut *tx)
        .await?;
        let expires = crate::database::timestamp(row.try_get("expires_at")?)?;
        Ok(assignment(current, permit.owner_id, permit.epoch, expires))
    }

    pub async fn submit_job(
        &self,
        authority: &Authority,
        command: Command<SubmitJob>,
    ) -> Result<Job> {
        let (mut tx, position) = self.begin_write().await?;
        check_identity(authority, &command)?;
        if let Some(result) = existing(
            &mut tx,
            authority,
            command.request_id,
            "submit_job",
            &command,
        )
        .await?
        {
            return Ok(result);
        }
        let result = submit(&mut tx, authority, &command, position, true).await?;
        tx.commit().await?;
        Ok(result)
    }
    pub async fn job(&self, authority: &Authority, id: Uuid) -> Result<Job> {
        job(&mut *self.pool.acquire().await?, authority, id).await
    }

    /// Oldest eligible queued jobs for the named processes within the authority's scope.
    /// Eligibility is rechecked by `claim_job`; this is a bounded, ordered candidate list.
    pub async fn queued_jobs(
        &self,
        authority: &Authority,
        processes: &[Process],
        limit: usize,
    ) -> Result<Vec<Uuid>> {
        let mut connection = self.pool.acquire().await?;
        let now = Utc::now().timestamp_millis();
        let (filter, values) = predicate(authority);
        let sql = format!(
            "SELECT j.id, j.spec FROM jobs j JOIN resource_scopes s ON s.tenant_id=j.tenant_id AND s.id=j.id WHERE {filter} AND j.state='queued' AND j.cancel_requested=0 AND j.deadline>$__NOW AND (j.ready_at IS NULL OR j.ready_at<=$__NOW) ORDER BY j.id LIMIT 500"
        );
        let index = values.len() + 1;
        let sql = sql.replace("$__NOW", &format!("${index}"));
        let mut query = sqlx::query(&sql);
        for value in values {
            query = query.bind(value);
        }
        query = query.bind(now);
        let mut candidates = Vec::new();
        for row in query.fetch_all(&mut *connection).await? {
            let spec: SubmitJob = serde_json::from_str(row.try_get("spec")?)?;
            if processes.contains(&spec.brief.process) && authority.scope.permits(&spec.brief.scope)
            {
                candidates.push((
                    crate::database::id(row.try_get("id")?)?,
                    spec.brief.scope.project_id,
                ));
            }
        }
        // Fair claiming: projects with fewer leased or running jobs come first; age breaks ties.
        let mut active: std::collections::HashMap<Option<Uuid>, i64> =
            std::collections::HashMap::new();
        let rows = sqlx::query(
            "SELECT spec FROM jobs WHERE tenant_id=$1 AND state IN ('leased','running')",
        )
        .bind(authority.tenant_id.to_string())
        .fetch_all(&mut *connection)
        .await?;
        for row in rows {
            let spec: SubmitJob = serde_json::from_str(row.try_get("spec")?)?;
            *active.entry(spec.brief.scope.project_id).or_default() += 1;
        }
        candidates.sort_by_key(|(id, project)| (active.get(project).copied().unwrap_or(0), *id));
        Ok(candidates
            .into_iter()
            .take(limit)
            .map(|(id, _)| id)
            .collect())
    }

    pub async fn claim_job(
        &self,
        authority: &Authority,
        id: Uuid,
        lease_seconds: u16,
    ) -> Result<Assignment> {
        if !(1..=300).contains(&lease_seconds) {
            return Err(Error::Invalid(
                "Lease must be between 1 and 300 seconds".into(),
            ));
        }
        let (mut tx, position) = self.begin_write().await?;
        require_write_scope(&mut tx, authority, id).await?;
        let query = if self.postgres {
            "SELECT id FROM jobs WHERE tenant_id=$1 AND id=$2 FOR UPDATE SKIP LOCKED"
        } else {
            "SELECT id FROM jobs WHERE tenant_id=$1 AND id=$2"
        };
        sqlx::query(query)
            .bind(authority.tenant_id.to_string())
            .bind(id.to_string())
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(Error::Conflict)?;
        let mut current = job(&mut tx, authority, id).await?;
        if current.state != JobState::Queued
            || current.cancel_requested
            || current.deadline <= position.recorded_at
            || current.ready_at.is_some_and(|at| at > position.recorded_at)
            || current.attempt >= u32::from(current.spec.max_attempts)
        {
            return Err(Error::Conflict);
        }
        let limits = budget(&mut tx, authority, current.spec.brief.limits.root_budget_id).await?;
        if current.spec.parent_id.is_some() {
            let row = sqlx::query("SELECT COUNT(*) AS count FROM jobs WHERE tenant_id=$1 AND root_id=$2 AND parent_id IS NOT NULL AND state IN ('leased','running')").bind(authority.tenant_id.to_string()).bind(current.root_id.to_string()).fetch_one(&mut *tx).await?;
            if row.try_get::<i64, _>("count")? >= i64::from(limits.max_child_concurrency) {
                return Err(Error::LimitExceeded);
            }
        }
        let prior = sqlx::query(
            "SELECT epoch,expires_at FROM session_leases WHERE tenant_id=$1 AND session_id=$2",
        )
        .bind(authority.tenant_id.to_string())
        .bind(&current.session_id)
        .fetch_optional(&mut *tx)
        .await?;
        let epoch = if let Some(prior) = prior {
            if prior.try_get::<i64, _>("expires_at")? > position.recorded_at.timestamp_millis() {
                return Err(Error::Conflict);
            }
            number(prior.try_get("epoch")?)?
                .checked_add(1)
                .ok_or(Error::LimitExceeded)?
        } else {
            1
        };
        let expires_at = (position.recorded_at + Duration::seconds(i64::from(lease_seconds)))
            .min(current.deadline);
        sqlx::query("INSERT INTO session_leases (tenant_id,session_id,job_id,owner_id,epoch,expires_at) VALUES ($1,$2,$3,$4,$5,$6) ON CONFLICT (tenant_id,session_id) DO UPDATE SET job_id=excluded.job_id,owner_id=excluded.owner_id,epoch=excluded.epoch,expires_at=excluded.expires_at")
            .bind(authority.tenant_id.to_string()).bind(&current.session_id).bind(id.to_string()).bind(authority.actor_id.to_string()).bind(i64::from(epoch)).bind(expires_at.timestamp_millis()).execute(&mut *tx).await?;
        current.attempt += 1;
        current.state = JobState::Leased;
        sqlx::query("UPDATE jobs SET state='leased',attempt=$1,ready_at=NULL,wait_reason=NULL WHERE tenant_id=$2 AND id=$3").bind(i64::from(current.attempt)).bind(authority.tenant_id.to_string()).bind(id.to_string()).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO job_attempts (tenant_id,job_id,attempt,owner_id,epoch,started_at) VALUES ($1,$2,$3,$4,$5,$6)").bind(authority.tenant_id.to_string()).bind(id.to_string()).bind(i64::from(current.attempt)).bind(authority.actor_id.to_string()).bind(i64::from(epoch)).bind(position.recorded_at.timestamp_millis()).execute(&mut *tx).await?;
        append_event(
            &mut tx,
            authority,
            id,
            "job_leased",
            position,
            current.spec.retain_until,
        )
        .await?;
        tx.commit().await?;
        Ok(assignment(current, authority.actor_id, epoch, expires_at))
    }

    pub async fn renew_assignment(
        &self,
        authority: &Authority,
        permit: &Fence,
        lease_seconds: u16,
    ) -> Result<Assignment> {
        if !(1..=300).contains(&lease_seconds) {
            return Err(Error::Invalid(
                "Lease must be between 1 and 300 seconds".into(),
            ));
        }
        let (mut tx, position) = self.begin_write().await?;
        let current = fence(&mut tx, authority, permit, position.recorded_at).await?;
        let expires = (position.recorded_at + Duration::seconds(i64::from(lease_seconds)))
            .min(current.deadline);
        sqlx::query("UPDATE session_leases SET expires_at=$1 WHERE tenant_id=$2 AND session_id=$3")
            .bind(expires.timestamp_millis())
            .bind(authority.tenant_id.to_string())
            .bind(&current.session_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(assignment(
            current,
            authority.actor_id,
            permit.epoch,
            expires,
        ))
    }

    pub async fn start_job(&self, authority: &Authority, permit: &Fence) -> Result<Job> {
        let (mut tx, position) = self.begin_write().await?;
        let mut current = fence(&mut tx, authority, permit, position.recorded_at).await?;
        crate::interaction::execution_gate(&mut tx, authority, &current).await?;
        if current.cancel_requested {
            return Err(Error::Conflict);
        }
        current.state = JobState::Running;
        sqlx::query("UPDATE jobs SET state='running' WHERE tenant_id=$1 AND id=$2")
            .bind(authority.tenant_id.to_string())
            .bind(current.id.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(current)
    }

    pub async fn wait_job(
        &self,
        authority: &Authority,
        permit: &Fence,
        reason: &str,
        ready_at: DateTime<Utc>,
    ) -> Result<()> {
        let (mut tx, position) = self.begin_write().await?;
        let current = fence(&mut tx, authority, permit, position.recorded_at).await?;
        if reason.trim().is_empty()
            || ready_at <= position.recorded_at
            || ready_at >= current.deadline
            || current.cancel_requested
        {
            return Err(Error::Invalid(
                "Wait needs a reason and wake time before the job deadline".into(),
            ));
        }
        sqlx::query("UPDATE jobs SET state='waiting',wait_reason=$1,ready_at=$2 WHERE tenant_id=$3 AND id=$4").bind(reason).bind(ready_at.timestamp_millis()).bind(authority.tenant_id.to_string()).bind(current.id.to_string()).execute(&mut *tx).await?;
        release(
            &mut tx,
            authority,
            &current,
            "waiting",
            position.recorded_at,
        )
        .await?;
        append_event(
            &mut tx,
            authority,
            current.id,
            "job_waiting",
            position,
            current.spec.retain_until,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn cancel_job(&self, authority: &Authority, id: Uuid) -> Result<()> {
        let (mut tx, position) = self.begin_write().await?;
        require_write_scope(&mut tx, authority, id).await?;
        let current = job(&mut tx, authority, id).await?;
        if current.state.terminal() || current.cancel_requested {
            return Ok(());
        }
        // Descendants share this root and may be running independently. Their owners
        // must observe cancellation and reconcile effects before terminal completion.
        let rows = sqlx::query("SELECT id,parent_id FROM jobs WHERE tenant_id=$1 AND root_id=$2")
            .bind(authority.tenant_id.to_string())
            .bind(current.root_id.to_string())
            .fetch_all(&mut *tx)
            .await?;
        let mut selected = std::collections::HashSet::from([id.to_string()]);
        loop {
            let before = selected.len();
            for row in &rows {
                if row
                    .try_get::<Option<String>, _>("parent_id")?
                    .is_some_and(|parent| selected.contains(&parent))
                {
                    selected.insert(row.try_get::<String, _>("id")?);
                }
            }
            if selected.len() == before {
                break;
            }
        }
        for id in selected {
            sqlx::query("UPDATE jobs SET cancel_requested=1 WHERE tenant_id=$1 AND id=$2 AND state NOT IN ('completed','partial','failed','cancelled')").bind(authority.tenant_id.to_string()).bind(id).execute(&mut *tx).await?;
        }
        append_event(
            &mut tx,
            authority,
            current.id,
            "cancellation_requested",
            position,
            current.spec.retain_until,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn complete_job(
        &self,
        authority: &Authority,
        request_id: Uuid,
        permit: &Fence,
        result: WorkResult,
    ) -> Result<Job> {
        let request = (permit.job_id, &result);
        let (mut tx, position) = self.begin_write().await?;
        if let Some(prior) =
            existing(&mut tx, authority, request_id, "complete_job", &request).await?
        {
            return Ok(prior);
        }
        let mut current = fence(&mut tx, authority, permit, position.recorded_at).await?;
        result.validate().map_err(|e| Error::Invalid(e.message))?;
        if current.cancel_requested || !current.spec.brief.scope.permits(&result.examined_scope) {
            return Err(Error::Conflict);
        }
        let unresolved = sqlx::query("SELECT COUNT(*) AS count FROM effect_requests WHERE tenant_id=$1 AND job_id=$2 AND state IN ('in_progress','outcome_unknown')").bind(authority.tenant_id.to_string()).bind(current.id.to_string()).fetch_one(&mut *tx).await?.try_get::<i64,_>("count")?;
        if unresolved != 0 && result.status == WorkStatus::Complete {
            return Err(Error::Conflict);
        }
        current.state = if result.status == WorkStatus::Complete {
            JobState::Completed
        } else {
            JobState::Partial
        };
        let children: i64 = sqlx::query("SELECT COUNT(*) AS count FROM jobs WHERE tenant_id=$1 AND root_id=$2 AND id<>$3 AND state NOT IN ('completed','partial','failed','cancelled')").bind(authority.tenant_id.to_string()).bind(current.root_id.to_string()).bind(current.id.to_string()).fetch_one(&mut *tx).await?.try_get("count")?;
        if current.id == current.root_id && children > 0 {
            return Err(Error::Conflict);
        }
        if let Some(id) = result.result_artifact {
            crate::artifacts::ready_artifact(&mut tx, authority, id).await?;
        }
        current.result = Some(result.clone());
        sqlx::query("UPDATE jobs SET state=$1,result=$2 WHERE tenant_id=$3 AND id=$4")
            .bind(tag(&current.state)?)
            .bind(serde_json::to_string(&result)?)
            .bind(authority.tenant_id.to_string())
            .bind(current.id.to_string())
            .execute(&mut *tx)
            .await?;
        release(
            &mut tx,
            authority,
            &current,
            "settled",
            position.recorded_at,
        )
        .await?;
        save(
            &mut tx,
            authority,
            request_id,
            current.id,
            "complete_job",
            &request,
            &current,
        )
        .await?;
        append_event(
            &mut tx,
            authority,
            current.id,
            "job_settled",
            position,
            current.spec.retain_until,
        )
        .await?;
        tx.commit().await?;
        Ok(current)
    }

    /// Reconcile expired owners, timers and requested cancellation. Unknown effects
    /// block re-execution until an explicit external observation resolves them.
    pub async fn recover_jobs(&self, authority: &Authority) -> Result<u32> {
        let (mut tx, position) = self.begin_write().await?;
        let (filter, values) = predicate(authority);
        let sql = format!(
            "SELECT j.id FROM jobs j JOIN resource_scopes s ON s.tenant_id=j.tenant_id AND s.id=j.id WHERE {filter} AND j.state NOT IN ('completed','partial','failed','cancelled') ORDER BY j.id LIMIT 1000"
        );
        let mut query = sqlx::query(&sql);
        for value in values {
            query = query.bind(value);
        }
        let rows = query.fetch_all(&mut *tx).await?;
        let mut count = 0;
        for row in rows {
            let id = crate::database::id(row.try_get("id")?)?;
            let current = job(&mut tx, authority, id).await?;
            if !authority.scope.permits(&current.spec.brief.scope) {
                continue;
            }
            let active = sqlx::query("SELECT expires_at FROM session_leases WHERE tenant_id=$1 AND session_id=$2 AND job_id=$3").bind(authority.tenant_id.to_string()).bind(&current.session_id).bind(id.to_string()).fetch_optional(&mut *tx).await?;
            if active.is_some_and(|r| {
                r.try_get::<i64, _>("expires_at")
                    .is_ok_and(|t| t > position.recorded_at.timestamp_millis())
            }) {
                continue;
            }
            sqlx::query("UPDATE effect_requests SET state='outcome_unknown' WHERE tenant_id=$1 AND job_id=$2 AND state='in_progress'").bind(authority.tenant_id.to_string()).bind(id.to_string()).execute(&mut *tx).await?;
            super::budgets::mark_usage_unknown(&mut tx, authority, id).await?;
            let unknown = sqlx::query("SELECT COUNT(*) AS count FROM effect_requests WHERE tenant_id=$1 AND job_id=$2 AND state='outcome_unknown'").bind(authority.tenant_id.to_string()).bind(id.to_string()).fetch_one(&mut *tx).await?.try_get::<i64,_>("count")? > 0;
            let pending_children: i64 = sqlx::query("SELECT COUNT(*) AS count FROM job_child_links l JOIN jobs c ON c.tenant_id=l.tenant_id AND c.id=l.child_id WHERE l.tenant_id=$1 AND l.parent_id=$2 AND l.waiting=1 AND c.state NOT IN ('completed','partial','failed','cancelled')")
                .bind(authority.tenant_id.to_string()).bind(id.to_string()).fetch_one(&mut *tx).await?.try_get("count")?;
            let children_ready =
                current.wait_reason.as_deref() == Some("children") && pending_children == 0;
            let state = if unknown {
                JobState::Waiting
            } else if current.cancel_requested {
                JobState::Cancelled
            } else if current.deadline <= position.recorded_at
                || current.attempt >= u32::from(current.spec.max_attempts)
            {
                JobState::Failed
            } else if current.ready_at.is_some_and(|at| at > position.recorded_at)
                && !children_ready
            {
                continue;
            } else {
                JobState::Queued
            };
            if unknown
                && current.state == JobState::Waiting
                && current.wait_reason.as_deref() == Some("external_outcome_unknown")
            {
                continue;
            }
            if state == current.state && !unknown {
                continue;
            }
            sqlx::query("UPDATE jobs SET state=$1,wait_reason=$2,ready_at=NULL WHERE tenant_id=$3 AND id=$4").bind(tag(&state)?).bind(if unknown { Some("external_outcome_unknown") } else { None }).bind(authority.tenant_id.to_string()).bind(id.to_string()).execute(&mut *tx).await?;
            release(
                &mut tx,
                authority,
                &current,
                if unknown {
                    "outcome_unknown"
                } else {
                    "interrupted"
                },
                position.recorded_at,
            )
            .await?;
            append_event(
                &mut tx,
                authority,
                id,
                "job_recovered",
                position,
                current.spec.retain_until,
            )
            .await?;
            count += 1;
        }
        tx.commit().await?;
        Ok(count)
    }
}

pub(crate) async fn submit(
    connection: &mut AnyConnection,
    authority: &Authority,
    command: &Command<SubmitJob>,
    position: memory_domain::records::CommitPosition,
    persist_receipt: bool,
) -> Result<Job> {
    command
        .check_authority(authority, position.recorded_at)
        .map_err(|e| Error::Invalid(e.message))?;
    let spec = &command.payload;
    let brief = &spec.brief;
    let mut envelope = serde_json::to_value(command)?;
    envelope["payload"] = serde_json::to_value(brief)?;
    serde_json::from_value::<Command<memory_domain::contracts::WorkBrief>>(envelope)?
        .validate()
        .map_err(|e| Error::Invalid(e.message))?;
    if spec.max_attempts == 0 || spec.retain_until <= command.deadline {
        return Err(Error::Invalid(
            "Job needs bounded attempts and retention beyond its deadline".into(),
        ));
    }
    let budget = budget(connection, authority, command.budget_id).await?;
    if !budget.scope.permits(&brief.scope) || command.deadline > budget.deadline {
        return Err(Error::Forbidden);
    }
    let id = Uuid::now_v7();
    let (root_id, depth) = if let Some(parent_id) = spec.parent_id {
        let parent = job(connection, authority, parent_id).await?;
        if parent.state.terminal()
            || parent.cancel_requested
            || parent.spec.brief.limits.root_budget_id != command.budget_id
            || !parent.spec.brief.scope.permits(&brief.scope)
            || command.deadline.timestamp_millis() > parent.deadline.timestamp_millis()
        {
            return Err(Error::Forbidden);
        }
        super::scoped::check_child(connection, authority, &parent, brief).await?;
        let depth = parent.depth.checked_add(1).ok_or(Error::LimitExceeded)?;
        if depth > budget.max_child_depth || depth > parent.spec.brief.limits.max_child_depth {
            return Err(Error::LimitExceeded);
        }
        (parent.root_id, depth)
    } else {
        (id, 0)
    };
    crate::policies::current_policy(connection, authority, &brief.policy).await?;
    for reference in &brief.inputs.memories {
        crate::memories::memory_version(connection, authority, reference).await?;
    }
    for reference in brief
        .inputs
        .sources
        .iter()
        .chain(&brief.capabilities.sources)
    {
        crate::sources::source_version(connection, authority, reference).await?;
    }
    for id in &brief.inputs.artifacts {
        crate::artifacts::ready_artifact(connection, authority, *id).await?;
    }
    write_scope(connection, authority, id, &brief.scope).await?;
    let session_id = if spec.parent_id.is_some() {
        format!("job:{id}")
    } else {
        format!("task:{}", brief.task_id)
    };
    let deleted =
        sqlx::query("SELECT session_id FROM deleted_sessions WHERE tenant_id=$1 AND session_id=$2")
            .bind(authority.tenant_id.to_string())
            .bind(&session_id)
            .fetch_optional(&mut *connection)
            .await?
            .is_some();
    let session_id = if deleted {
        format!("job:{id}")
    } else {
        session_id
    };
    let prior = sqlx::query(
        "SELECT spec FROM jobs WHERE tenant_id=$1 AND session_id=$2 ORDER BY id DESC LIMIT 1",
    )
    .bind(authority.tenant_id.to_string())
    .bind(&session_id)
    .fetch_optional(&mut *connection)
    .await?;
    let session_id = if let Some(prior) = prior {
        let prior: SubmitJob = serde_json::from_str(prior.try_get("spec")?)?;
        if !crate::access::same_scope(&prior.brief.scope, &brief.scope)
            || prior.brief.profile != brief.profile
        {
            format!("job:{id}")
        } else {
            session_id
        }
    } else {
        session_id
    };
    let operation_id = Uuid::now_v7().to_string();
    sqlx::query("INSERT INTO jobs (tenant_id,id,root_id,parent_id,budget_id,session_id,operation_id,state,attempt,deadline,spec,depth,retain_until) VALUES ($1,$2,$3,$4,$5,$6,$7,'queued',0,$8,$9,$10,$11)")
        .bind(authority.tenant_id.to_string()).bind(id.to_string()).bind(root_id.to_string()).bind(spec.parent_id.map(|v|v.to_string())).bind(command.budget_id.to_string()).bind(&session_id).bind(&operation_id).bind(command.deadline.timestamp_millis()).bind(serde_json::to_string(spec)?).bind(i64::from(depth)).bind(spec.retain_until.timestamp_millis()).execute(&mut *connection).await?;
    let result = job(connection, authority, id).await?;
    if persist_receipt {
        save(
            connection,
            authority,
            command.request_id,
            id,
            "submit_job",
            command,
            &result,
        )
        .await?;
    }
    append_event(
        connection,
        authority,
        id,
        "job_accepted",
        position,
        spec.retain_until,
    )
    .await?;
    Ok(result)
}
pub(crate) async fn release(
    connection: &mut AnyConnection,
    authority: &Authority,
    job: &Job,
    outcome: &str,
    now: DateTime<Utc>,
) -> Result<()> {
    sqlx::query("UPDATE session_leases SET expires_at=$1 WHERE tenant_id=$2 AND session_id=$3 AND job_id=$4").bind(now.timestamp_millis()).bind(authority.tenant_id.to_string()).bind(&job.session_id).bind(job.id.to_string()).execute(&mut *connection).await?;
    sqlx::query("UPDATE job_attempts SET ended_at=$1,outcome=$2 WHERE tenant_id=$3 AND job_id=$4 AND attempt=$5 AND ended_at IS NULL").bind(now.timestamp_millis()).bind(outcome).bind(authority.tenant_id.to_string()).bind(job.id.to_string()).bind(i64::from(job.attempt)).execute(connection).await?;
    Ok(())
}

impl Store {
    pub async fn acknowledge_cancellation(
        &self,
        authority: &Authority,
        permit: &Fence,
    ) -> Result<Job> {
        let (mut tx, position) = self.begin_write().await?;
        let mut current = fence(&mut tx, authority, permit, position.recorded_at).await?;
        if !current.cancel_requested {
            return Err(Error::Conflict);
        }
        let unresolved: i64 = sqlx::query("SELECT COUNT(*) AS count FROM effect_requests WHERE tenant_id=$1 AND job_id=$2 AND state IN ('in_progress','outcome_unknown')").bind(authority.tenant_id.to_string()).bind(current.id.to_string()).fetch_one(&mut *tx).await?.try_get("count")?;
        if unresolved > 0 {
            return Err(Error::Conflict);
        }
        current.state = JobState::Cancelled;
        sqlx::query("UPDATE jobs SET state='cancelled' WHERE tenant_id=$1 AND id=$2")
            .bind(authority.tenant_id.to_string())
            .bind(current.id.to_string())
            .execute(&mut *tx)
            .await?;
        release(
            &mut tx,
            authority,
            &current,
            "cancelled",
            position.recorded_at,
        )
        .await?;
        append_event(
            &mut tx,
            authority,
            current.id,
            "job_cancelled",
            position,
            current.spec.retain_until,
        )
        .await?;
        tx.commit().await?;
        Ok(current)
    }
}
