mod consolidation;
mod maintenance;
mod qualification;

use crate::{Error, Result, Store, access::predicate, database::id};
use chrono::{DateTime, Duration, Utc};
use memory_domain::{
    contracts::{Authority, Command, ConfigRef, Scope, WireVersion, WorkBrief},
    coordination::{Job, SubmitJob},
    scheduling::ScheduledWorkMark,
};
use sqlx::{Acquire, AnyConnection, Row};
use uuid::Uuid;

/// Marks a trigger the scheduler examined and could not schedule; it is not retried.
const UNSCHEDULABLE: &str = "unschedulable";
/// A scheduled consolidation/qualification/maintenance job gets this long from scheduling,
/// inside the task's budget deadline.
const SCHEDULING_WINDOW: Duration = Duration::minutes(30);

/// What a sweep decided about one trigger, recorded in `scheduled_work` either way: a submitted
/// job, or why it could not be scheduled (with the causal job responsible, when there is one).
enum ScheduleOutcome {
    Scheduled(Uuid),
    Unschedulable(String),
}
impl ScheduleOutcome {
    fn from_error(error: Error) -> Result<Self> {
        match error {
            Error::Invalid(reason) => Ok(Self::Unschedulable(reason)),
            Error::Forbidden | Error::LimitExceeded => {
                Ok(Self::Unschedulable(UNSCHEDULABLE.into()))
            }
            other => Err(other),
        }
    }
}

/// A job's scope must carry its own task dimension (`Command<WorkBrief>::validate`). Reuse the
/// scope's task if it already has one (so records already narrowed to it stay readable); invent a
/// fresh one otherwise. A resource with no task dimension of its own is readable regardless.
fn scoped_task(scope: &Scope) -> (Uuid, Scope) {
    match scope.task_id {
        Some(existing) => (existing, scope.clone()),
        None => {
            let fresh = Uuid::now_v7();
            (
                fresh,
                Scope {
                    task_id: Some(fresh),
                    ..scope.clone()
                },
            )
        }
    }
}

struct ScheduleTrigger {
    kind: &'static str,
    id: Uuid,
    dedupe_key: String,
    causal: Option<Uuid>,
}
type CohortMember = (Uuid, u32, String);

impl Store {
    /// Reads what the scheduler recorded for one trigger: the job it submitted, an unschedulable
    /// mark with its reason, or nothing yet.
    pub async fn scheduled_work(
        &self,
        authority: &Authority,
        trigger_kind: &str,
        trigger_id: Uuid,
    ) -> Result<Option<ScheduledWorkMark>> {
        let row = sqlx::query(
            "SELECT dedupe_key,job_id,unschedulable,causal_origin_job_id,scheduled_at FROM scheduled_work \
             WHERE tenant_id=$1 AND trigger_kind=$2 AND trigger_id=$3",
        )
        .bind(authority.tenant_id.to_string())
        .bind(trigger_kind)
        .bind(trigger_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else { return Ok(None) };
        Ok(Some(ScheduledWorkMark {
            trigger_kind: trigger_kind.to_string(),
            trigger_id,
            dedupe_key: row.try_get("dedupe_key")?,
            job_id: row
                .try_get::<Option<String>, _>("job_id")?
                .map(|v| id(&v))
                .transpose()?,
            unschedulable: row.try_get("unschedulable")?,
            causal_origin_job_id: row
                .try_get::<Option<String>, _>("causal_origin_job_id")?
                .map(|v| id(&v))
                .transpose()?,
            scheduled_at: crate::database::timestamp(row.try_get("scheduled_at")?)?,
        }))
    }

    async fn mark_scheduled(
        connection: &mut AnyConnection,
        authority: &Authority,
        trigger: &ScheduleTrigger,
        outcome: ScheduleOutcome,
    ) -> Result<()> {
        let (job_id, reason) = match outcome {
            ScheduleOutcome::Scheduled(id) => (Some(id.to_string()), None),
            ScheduleOutcome::Unschedulable(reason) => (None, Some(reason)),
        };
        sqlx::query(
            "INSERT INTO scheduled_work(tenant_id,trigger_kind,trigger_id,dedupe_key,job_id,unschedulable,causal_origin_job_id,scheduled_at) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT (tenant_id,trigger_kind,trigger_id) DO NOTHING",
        )
        .bind(authority.tenant_id.to_string())
        .bind(trigger.kind)
        .bind(trigger.id.to_string())
        .bind(&trigger.dedupe_key)
        .bind(job_id)
        .bind(reason)
        .bind(trigger.causal.map(|j| j.to_string()))
        .bind(Utc::now().timestamp_millis())
        .execute(connection)
        .await?;
        Ok(())
    }

    /// Submission, cohort membership and the trigger mark commit together. The store's write
    /// clock serializes competing sweeps; recheck ownership after acquiring it, not before.
    async fn commit_schedule(
        &self,
        authority: &Authority,
        trigger: ScheduleTrigger,
        plan: Result<Command<SubmitJob>>,
        cohort: Option<(&ConfigRef, &[CohortMember])>,
    ) -> Result<u32> {
        let (mut tx, position) = self.begin_write().await?;
        let exists = sqlx::query(
            "SELECT 1 FROM scheduled_work WHERE tenant_id=$1 AND trigger_kind=$2 AND trigger_id=$3",
        )
        .bind(authority.tenant_id.to_string())
        .bind(trigger.kind)
        .bind(trigger.id.to_string())
        .fetch_optional(&mut *tx)
        .await?
        .is_some();
        if exists {
            return Ok(0);
        }
        if let Some((_, members)) = cohort {
            for (memory_id, _, _) in members {
                let claimed = sqlx::query("SELECT 1 FROM consolidation_cohort_members WHERE tenant_id=$1 AND memory_id=$2")
                    .bind(authority.tenant_id.to_string()).bind(memory_id.to_string())
                    .fetch_optional(&mut *tx).await?.is_some();
                if claimed {
                    return Ok(0);
                }
            }
        }
        let outcome = match plan {
            Ok(command) => {
                // Invalid submissions may have written rows before failing. Roll those back while
                // still allowing the outer transaction to record the unschedulable reason.
                let mut submission = tx.begin().await?;
                match crate::coordinator::jobs::submit(
                    &mut submission,
                    authority,
                    &command,
                    position,
                    true,
                )
                .await
                {
                    Ok(job) => {
                        submission.commit().await?;
                        ScheduleOutcome::Scheduled(job.id)
                    }
                    Err(error) => {
                        submission.rollback().await?;
                        ScheduleOutcome::from_error(error)?
                    }
                }
            }
            Err(error) => ScheduleOutcome::from_error(error)?,
        };
        let scheduled = u32::from(matches!(outcome, ScheduleOutcome::Scheduled(_)));
        if let Some((policy, members)) = cohort {
            for (memory_id, _, _) in members {
                sqlx::query("INSERT INTO consolidation_cohort_members(tenant_id,policy_id,memory_id,cohort_id) VALUES($1,$2,$3,$4)")
                    .bind(authority.tenant_id.to_string()).bind(policy.id.to_string())
                    .bind(memory_id.to_string()).bind(trigger.id.to_string())
                    .execute(&mut *tx).await?;
            }
        }
        Self::mark_scheduled(&mut tx, authority, &trigger, outcome).await?;
        tx.commit().await?;
        Ok(scheduled)
    }

    async fn record_unschedulable(
        &self,
        authority: &Authority,
        kind: &'static str,
        id: Uuid,
        dedupe_key: &str,
        reason: &str,
        causal: Option<Uuid>,
    ) -> Result<()> {
        self.commit_schedule(
            authority,
            ScheduleTrigger {
                kind,
                id,
                dedupe_key: dedupe_key.into(),
                causal,
            },
            Err(Error::Invalid(reason.into())),
            None,
        )
        .await?;
        Ok(())
    }

    /// The most recently submitted root task job whose scope permits `scope`, used as the budget
    /// source for scheduled work with no single owning task (a consolidation cohort spans many
    /// contributing tasks; a maintenance trigger may come from a human correction, not a job).
    /// Job ids are UUIDv7, so ordering by id descending is ordering by recency. Checked against
    /// the job's *budget* scope (a resource of its own), not the job's own brief scope: many
    /// task-scoped jobs commonly share one project-scoped budget approved ahead of time.
    async fn recent_contributing_job(&self, authority: &Authority, scope: &Scope) -> Result<Job> {
        let (filter, values) = predicate(authority);
        let sql = format!(
            "SELECT j.id FROM jobs j JOIN resource_scopes s ON s.tenant_id=j.tenant_id AND s.id=j.id \
             WHERE {filter} AND j.id=j.root_id ORDER BY j.id DESC LIMIT 200"
        );
        let mut query = sqlx::query(&sql);
        for v in &values {
            query = query.bind(v);
        }
        let rows = query.fetch_all(&self.pool).await?;
        for row in rows {
            let candidate = id(row.try_get("id")?)?;
            let job = self.job(authority, candidate).await?;
            let budget = crate::coordinator::budgets::budget(
                &mut *self.pool.acquire().await?,
                authority,
                job.spec.brief.limits.root_budget_id,
            )
            .await?;
            if budget.scope.permits(scope) {
                return Ok(job);
            }
        }
        Err(Error::Invalid(
            "No contributing task budget covers this scope".into(),
        ))
    }

    /// Deadline for a scheduled job derived from a chosen budget: bounded by the scheduling
    /// window and the budget's own deadline, rejected if that leaves no working time.
    async fn scheduled_deadline(
        &self,
        authority: &Authority,
        budget_id: Uuid,
    ) -> Result<DateTime<Utc>> {
        let budget = crate::coordinator::budgets::budget(
            &mut *self.pool.acquire().await?,
            authority,
            budget_id,
        )
        .await?;
        let now = Utc::now();
        let deadline = (now + SCHEDULING_WINDOW).min(budget.deadline);
        if deadline <= now + Duration::seconds(30) {
            return Err(Error::Invalid(
                "Budget deadline leaves no time for scheduled work".into(),
            ));
        }
        Ok(deadline)
    }

    fn scheduled_command(
        authority: &Authority,
        request_id: Uuid,
        brief: WorkBrief,
        deadline: DateTime<Utc>,
        retain_until: DateTime<Utc>,
    ) -> Command<SubmitJob> {
        let scope = brief.scope.clone();
        let budget_id = brief.limits.root_budget_id;
        Command {
            request_id,
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
            budget_id,
            lease_epoch: None,
            payload: SubmitJob {
                brief,
                parent_id: None,
                max_attempts: 2,
                retain_until,
            },
        }
    }
}
