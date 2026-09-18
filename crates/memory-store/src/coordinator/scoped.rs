use super::{
    events::append_event,
    fence, job,
    jobs::{release, submit},
    receipts::{existing, save},
};
use crate::{Error, Result, Store};
use chrono::{DateTime, Utc};
use memory_domain::{
    contracts::{Authority, Command, WireVersion, WorkBrief},
    coordination::{Fence, Job, JobState, SubmitJob},
};
use sqlx::{AnyConnection, Row};
use uuid::Uuid;

pub(crate) async fn check_child(
    connection: &mut AnyConnection,
    auth: &Authority,
    parent: &Job,
    child: &WorkBrief,
) -> Result<()> {
    for id in &child.inputs.artifacts {
        if !parent.spec.brief.inputs.artifacts.contains(id) {
            // A job can delegate its own governed artifacts, but not arbitrary readable artifacts.
            let owned = sqlx::query("SELECT artifact_id FROM job_artifacts WHERE tenant_id=$1 AND job_id=$2 AND artifact_id=$3")
                .bind(auth.tenant_id.to_string()).bind(parent.id.to_string()).bind(id.to_string()).fetch_optional(&mut *connection).await?.is_some();
            if !owned {
                return Err(Error::Forbidden);
            }
        }
    }
    let p = &parent.spec.brief;
    let permitted = p.scope.permits(&child.scope)
        && child.task_id == p.task_id
        && child.policy == p.policy
        && child.profile == p.profile
        && child.retention_policy == p.retention_policy
        && child.disclosure_policy == p.disclosure_policy
        && child.evidence_cutoff <= p.evidence_cutoff
        && child
            .capabilities
            .tools
            .iter()
            .all(|v| p.capabilities.tools.contains(v))
        && child
            .capabilities
            .queries
            .iter()
            .all(|v| p.capabilities.queries.contains(v))
        && child
            .capabilities
            .sources
            .iter()
            .all(|v| p.capabilities.sources.contains(v))
        && child
            .inputs
            .sources
            .iter()
            .all(|v| p.capabilities.sources.contains(v))
        && child
            .inputs
            .memories
            .iter()
            .all(|v| p.inputs.memories.contains(v))
        && child.limits.root_budget_id == p.limits.root_budget_id
        && child.limits.max_tokens <= p.limits.max_tokens
        && child.limits.max_provider_attempts <= p.limits.max_provider_attempts
        && child.limits.max_output_bytes <= p.limits.max_output_bytes
        && child.limits.max_child_depth <= p.limits.max_child_depth
        && child.limits.max_child_concurrency <= p.limits.max_child_concurrency;
    if permitted {
        Ok(())
    } else {
        Err(Error::Forbidden)
    }
}

impl Store {
    pub async fn spawn_child(
        &self,
        auth: &Authority,
        permit: &Fence,
        request_id: Uuid,
        brief: WorkBrief,
        deadline: DateTime<Utc>,
        reuse: Option<Uuid>,
    ) -> Result<Job> {
        let (mut tx, position) = self.begin_write().await?;
        let parent = fence(&mut tx, auth, permit, position.recorded_at).await?;
        if parent.cancel_requested {
            return Err(Error::Forbidden);
        }
        check_child(&mut tx, auth, &parent, &brief).await?;
        if deadline <= position.recorded_at
            || deadline.timestamp_millis() > parent.deadline.timestamp_millis()
        {
            return Err(Error::Forbidden);
        }
        let request =
            serde_json::json!({"parent":parent.id,"brief":brief,"deadline":deadline,"reuse":reuse});
        if let Some(prior) =
            existing::<Job>(&mut tx, auth, request_id, "spawn_child", &request).await?
        {
            let current = job(&mut tx, auth, prior.id).await?;
            check_child_access(&mut tx, auth, &current).await?;
            return Ok(current);
        }
        let child = if let Some(id) = reuse {
            let prior = job(&mut tx, auth, id).await?;
            // Reuse is explicit and restricted to a completed, still-retained result
            // with the same interpretation and evidence basis. No hidden semantic match.
            let mut requested = serde_json::to_value(&brief)?;
            let mut original = serde_json::to_value(&prior.spec.brief)?;
            requested.as_object_mut().unwrap().remove("limits");
            original.as_object_mut().unwrap().remove("limits");
            if prior.state != JobState::Completed
                || prior.spec.retain_until <= position.recorded_at
                || requested != original
            {
                return Err(Error::Conflict);
            }
            check_child_access(&mut tx, auth, &prior).await?;
            prior
        } else {
            let count: i64 = sqlx::query("SELECT COUNT(*) AS count FROM jobs WHERE tenant_id=$1 AND parent_id=$2 AND state NOT IN ('completed','partial','failed','cancelled')")
                .bind(auth.tenant_id.to_string()).bind(parent.id.to_string()).fetch_one(&mut *tx).await?.try_get("count")?;
            if count >= i64::from(parent.spec.brief.limits.max_child_concurrency) {
                return Err(Error::LimitExceeded);
            }
            let command = Command {
                command_version: WireVersion::V1,
                request_id: Uuid::now_v7(),
                tenant_id: auth.tenant_id,
                actor_id: auth.actor_id,
                scope: brief.scope.clone(),
                job_id: Some(parent.id),
                session_id: None,
                operation_id: None,
                lane_id: None,
                invocation_id: None,
                expected_revisions: vec![],
                deadline,
                budget_id: brief.limits.root_budget_id,
                lease_epoch: None,
                payload: SubmitJob {
                    brief,
                    parent_id: Some(parent.id),
                    max_attempts: parent.spec.max_attempts,
                    retain_until: parent.spec.retain_until,
                },
            };
            submit(&mut tx, auth, &command, position, false).await?
        };
        sqlx::query("INSERT INTO job_child_links(tenant_id,parent_id,child_id) VALUES($1,$2,$3) ON CONFLICT DO NOTHING")
            .bind(auth.tenant_id.to_string()).bind(parent.id.to_string()).bind(child.id.to_string()).execute(&mut *tx).await?;
        save(
            &mut tx,
            auth,
            request_id,
            parent.id,
            "spawn_child",
            &request,
            &child,
        )
        .await?;
        tx.commit().await?;
        Ok(child)
    }
    pub async fn child_jobs(
        &self,
        auth: &Authority,
        permit: &Fence,
        ids: &[Uuid],
    ) -> Result<Vec<Job>> {
        let (mut tx, position) = self.begin_write().await?;
        fence(&mut tx, auth, permit, position.recorded_at).await?;
        if ids.len() > 32 {
            return Err(Error::LimitExceeded);
        }
        let mut jobs = vec![];
        for id in ids {
            let linked = sqlx::query("SELECT child_id FROM job_child_links WHERE tenant_id=$1 AND parent_id=$2 AND child_id=$3")
                .bind(auth.tenant_id.to_string()).bind(permit.job_id.to_string()).bind(id.to_string()).fetch_optional(&mut *tx).await?;
            if linked.is_none() {
                return Err(Error::Forbidden);
            }
            let child = job(&mut tx, auth, *id).await?;
            check_child_access(&mut tx, auth, &child).await?;
            jobs.push(child);
        }
        Ok(jobs)
    }
    pub async fn wait_children(
        &self,
        auth: &Authority,
        permit: &Fence,
        ids: &[Uuid],
        ready_at: DateTime<Utc>,
    ) -> Result<()> {
        // Validate membership, then recheck the fence in the transaction that releases ownership.
        self.child_jobs(auth, permit, ids).await?;
        let (mut tx, position) = self.begin_write().await?;
        let parent = fence(&mut tx, auth, permit, position.recorded_at).await?;
        if ids.is_empty()
            || parent.cancel_requested
            || ready_at <= position.recorded_at
            || ready_at >= parent.deadline
        {
            return Err(Error::Invalid("Invalid child wait".into()));
        }
        sqlx::query("UPDATE job_child_links SET waiting=0 WHERE tenant_id=$1 AND parent_id=$2")
            .bind(auth.tenant_id.to_string())
            .bind(parent.id.to_string())
            .execute(&mut *tx)
            .await?;
        for id in ids {
            sqlx::query("UPDATE job_child_links SET waiting=1 WHERE tenant_id=$1 AND parent_id=$2 AND child_id=$3").bind(auth.tenant_id.to_string()).bind(parent.id.to_string()).bind(id.to_string()).execute(&mut *tx).await?;
        }
        sqlx::query("UPDATE jobs SET state='waiting',wait_reason='children',ready_at=$1 WHERE tenant_id=$2 AND id=$3").bind(ready_at.timestamp_millis()).bind(auth.tenant_id.to_string()).bind(parent.id.to_string()).execute(&mut *tx).await?;
        release(&mut tx, auth, &parent, "waiting", position.recorded_at).await?;
        append_event(
            &mut tx,
            auth,
            parent.id,
            "job_waiting",
            position,
            parent.spec.retain_until,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn input_artifact(&self, auth: &Authority, permit: &Fence, id: Uuid) -> Result<()> {
        let (mut tx, position) = self.begin_write().await?;
        let parent = fence(&mut tx, auth, permit, position.recorded_at).await?;
        if parent.cancel_requested {
            return Err(Error::Forbidden);
        }
        let own = sqlx::query("SELECT artifact_id FROM job_artifacts WHERE tenant_id=$1 AND job_id=$2 AND artifact_id=$3").bind(auth.tenant_id.to_string()).bind(parent.id.to_string()).bind(id.to_string()).fetch_optional(&mut *tx).await?.is_some();
        let rows = sqlx::query("SELECT c.result FROM job_child_links l JOIN jobs c ON c.tenant_id=l.tenant_id AND c.id=l.child_id WHERE l.tenant_id=$1 AND l.parent_id=$2 AND c.result IS NOT NULL").bind(auth.tenant_id.to_string()).bind(parent.id.to_string()).fetch_all(&mut *tx).await?;
        let child = rows.iter().any(|r| {
            serde_json::from_str::<memory_domain::contracts::WorkResult>(r.get("result"))
                .is_ok_and(|r| r.result_artifact == Some(id))
        });
        if !own && !child && !parent.spec.brief.inputs.artifacts.contains(&id) {
            return Err(Error::Forbidden);
        }
        crate::artifacts::ready_artifact(&mut tx, auth, id).await?;
        Ok(())
    }
    pub async fn link_job_artifact(
        &self,
        auth: &Authority,
        permit: &Fence,
        id: Uuid,
    ) -> Result<()> {
        let (mut tx, position) = self.begin_write().await?;
        fence(&mut tx, auth, permit, position.recorded_at).await?;
        crate::artifacts::ready_artifact(&mut tx, auth, id).await?;
        sqlx::query("INSERT INTO job_artifacts(tenant_id,job_id,artifact_id) VALUES($1,$2,$3) ON CONFLICT DO NOTHING").bind(auth.tenant_id.to_string()).bind(permit.job_id.to_string()).bind(id.to_string()).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
}

// Cached findings must still be readable when consumed, including their evidence.
async fn check_child_access(
    connection: &mut AnyConnection,
    auth: &Authority,
    child: &Job,
) -> Result<()> {
    let inputs =
        std::iter::once(&child.spec.brief.inputs).chain(child.result.iter().map(|r| &r.inputs));
    for input in inputs {
        for r in &input.memories {
            crate::memories::memory_version(connection, auth, r).await?;
        }
        for r in &input.sources {
            crate::sources::source_version(connection, auth, r).await?;
        }
        for id in &input.artifacts {
            crate::artifacts::ready_artifact(connection, auth, *id).await?;
        }
    }
    if let Some(result) = &child.result {
        let artifact = result.result_artifact.ok_or(Error::Unavailable)?;
        for id in std::iter::once(&artifact).chain(&result.child_outputs) {
            crate::artifacts::ready_artifact(connection, auth, *id).await?;
        }
    }
    Ok(())
}
