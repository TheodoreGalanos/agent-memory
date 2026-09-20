//! ABOUTME: Durable rows -> Host timer -> idempotent submit_job for consolidation cohorts, procedure
//! ABOUTME: qualification and maintenance, mirroring the formation scheduler in `experience.rs`.
use crate::{Error, Result, Store, access::predicate, database::id};
use chrono::{DateTime, Duration, Utc};
use memory_domain::{
    contracts::{
        Authority, Command, ConfigRef, Process, Scope, WireVersion, WorkBrief, WorkInputs,
    },
    coordination::{Job, SubmitJob},
    records::{Qualification, RecordDraft},
    scheduling::ScheduledWorkMark,
};
use sqlx::Row;
use std::num::NonZeroU32;
use uuid::Uuid;

/// Marks a trigger the scheduler examined and could not schedule; it is not retried.
const UNSCHEDULABLE: &str = "unschedulable";
/// A scheduled consolidation/qualification/maintenance job gets this long from scheduling,
/// inside the task's budget deadline.
const SCHEDULING_WINDOW: Duration = Duration::minutes(30);
/// Bound on how many memory_versions candidates a single consolidation sweep inspects.
const CANDIDATE_SCAN: i64 = 500;

/// What a sweep decided about one trigger, recorded in `scheduled_work` either way: a submitted
/// job, or why it could not be scheduled (with the causal job responsible, when there is one).
#[derive(Default)]
struct ScheduleOutcome {
    job_id: Option<Uuid>,
    unschedulable: Option<String>,
    causal_origin_job_id: Option<Uuid>,
}
impl ScheduleOutcome {
    fn scheduled(job_id: Uuid) -> Self {
        Self {
            job_id: Some(job_id),
            ..Default::default()
        }
    }
    fn unschedulable(reason: impl Into<String>) -> Self {
        Self {
            unschedulable: Some(reason.into()),
            ..Default::default()
        }
    }
    fn from_result(outcome: Result<Uuid>) -> Result<Self> {
        match outcome {
            Ok(job_id) => Ok(Self::scheduled(job_id)),
            Err(Error::Invalid(reason)) => Ok(Self::unschedulable(reason)),
            Err(Error::Forbidden) | Err(Error::LimitExceeded) => {
                Ok(Self::unschedulable(UNSCHEDULABLE))
            }
            Err(e) => Err(e),
        }
    }
    fn causal(mut self, job_id: Option<Uuid>) -> Self {
        self.causal_origin_job_id = job_id;
        self
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
        &self,
        authority: &Authority,
        trigger_kind: &str,
        trigger_id: Uuid,
        dedupe_key: &str,
        outcome: ScheduleOutcome,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO scheduled_work(tenant_id,trigger_kind,trigger_id,dedupe_key,job_id,unschedulable,causal_origin_job_id,scheduled_at) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT (tenant_id,trigger_kind,trigger_id) DO NOTHING",
        )
        .bind(authority.tenant_id.to_string())
        .bind(trigger_kind)
        .bind(trigger_id.to_string())
        .bind(dedupe_key)
        .bind(outcome.job_id.map(|j| j.to_string()))
        .bind(outcome.unschedulable)
        .bind(outcome.causal_origin_job_id.map(|j| j.to_string()))
        .bind(Utc::now().timestamp_millis())
        .execute(&self.pool)
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

    async fn submit_scheduled(
        &self,
        authority: &Authority,
        request_id: Uuid,
        brief: WorkBrief,
        deadline: DateTime<Utc>,
        retain_until: DateTime<Utc>,
    ) -> Result<Job> {
        let scope = brief.scope.clone();
        let budget_id = brief.limits.root_budget_id;
        self.submit_job(
            authority,
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
            },
        )
        .await
    }

    /// Groups retained, not-yet-consolidated candidate records by policy and, once a policy's
    /// group reaches its configured minimum, submits one bounded consolidation job over the
    /// oldest members and records them as covered so a repeated sweep does not reform the cohort.
    /// A later sweep may still form a new cohort from records recorded since.
    pub async fn schedule_consolidation(&self, authority: &Authority, limit: u16) -> Result<u32> {
        let limit = limit.clamp(1, 100);
        let (filter, values) = predicate(authority);
        let sql = format!(
            "SELECT mv.id,mv.revision,mv.data FROM memory_versions mv \
             JOIN resource_scopes s ON s.tenant_id=mv.tenant_id AND s.id=mv.id \
             WHERE {filter} AND mv.recorded_to IS NULL AND mv.availability='routine' \
             AND NOT EXISTS (SELECT 1 FROM consolidation_cohort_members m WHERE m.tenant_id=mv.tenant_id AND m.memory_id=mv.id) \
             ORDER BY mv.recorded_from LIMIT {CANDIDATE_SCAN}"
        );
        let mut query = sqlx::query(&sql);
        for v in &values {
            query = query.bind(v);
        }
        let rows = query.fetch_all(&self.pool).await?;
        type Candidate = (Uuid, u32, String);
        let mut groups: std::collections::BTreeMap<(Uuid, u32), Vec<Candidate>> =
            std::collections::BTreeMap::new();
        for row in rows {
            let memory_id = id(row.try_get("id")?)?;
            let revision: u32 = crate::database::number(row.try_get("revision")?)?;
            let data: String = row.try_get("data")?;
            let record: RecordDraft = serde_json::from_str(&data)?;
            if !matches!(record.qualification, Qualification::Candidate) {
                continue;
            }
            groups
                .entry((
                    record.decision.policy.id,
                    record.decision.policy.revision.get(),
                ))
                .or_default()
                .push((memory_id, revision, record.label.clone()));
        }
        let mut cohorts = vec![];
        for ((policy_id, policy_revision), members) in groups {
            let config_ref = ConfigRef {
                id: policy_id,
                revision: NonZeroU32::new(policy_revision)
                    .ok_or(Error::Invalid("Policy revision must be positive".into()))?,
                label: String::new(),
            };
            let policy = match self.policy(authority, &config_ref).await {
                Ok(p) => p,
                Err(Error::NotFound | Error::Forbidden) => continue,
                Err(e) => return Err(e),
            };
            let Some(rules) = &policy.policy.consolidation else {
                continue;
            };
            if members.len() < rules.min_cohort_size as usize {
                continue;
            }
            let mut members = members;
            members.truncate(rules.max_cohort_size as usize);
            cohorts.push((
                policy.policy.scheduling_priority,
                policy.reference.clone(),
                policy.scope,
                members,
            ));
        }
        cohorts.sort_by(|a, b| b.0.cmp(&a.0));
        let mut scheduled = 0u32;
        for (_, policy_ref, policy_scope, members) in cohorts.into_iter().take(usize::from(limit)) {
            let cohort_id = Uuid::now_v7();
            let dedupe_key = members
                .iter()
                .map(|(id, revision, _)| format!("{id}:{revision}"))
                .collect::<Vec<_>>()
                .join(",");
            let outcome = ScheduleOutcome::from_result(
                self.schedule_one_consolidation(
                    authority,
                    cohort_id,
                    &policy_ref,
                    &policy_scope,
                    &members,
                )
                .await,
            )?;
            if outcome.job_id.is_some() {
                scheduled += 1;
            }
            for (memory_id, _, _) in &members {
                sqlx::query(
                    "INSERT INTO consolidation_cohort_members(tenant_id,policy_id,memory_id,cohort_id) VALUES($1,$2,$3,$4) \
                     ON CONFLICT (tenant_id,policy_id,memory_id) DO NOTHING",
                )
                .bind(authority.tenant_id.to_string())
                .bind(policy_ref.id.to_string())
                .bind(memory_id.to_string())
                .bind(cohort_id.to_string())
                .execute(&self.pool)
                .await?;
            }
            self.mark_scheduled(authority, "consolidation", cohort_id, &dedupe_key, outcome)
                .await?;
        }
        Ok(scheduled)
    }

    async fn schedule_one_consolidation(
        &self,
        authority: &Authority,
        cohort_id: Uuid,
        policy_ref: &ConfigRef,
        policy_scope: &Scope,
        members: &[(Uuid, u32, String)],
    ) -> Result<Uuid> {
        let (task_id, scope) = scoped_task(policy_scope);
        let origin = self.recent_contributing_job(authority, &scope).await?;
        let deadline = self
            .scheduled_deadline(authority, origin.spec.brief.limits.root_budget_id)
            .await?;
        let mut brief = origin.spec.brief.clone();
        brief.process = Process::Consolidation;
        brief.purpose = format!("consolidate cohort {cohort_id}");
        brief.task_id = task_id;
        brief.scope = scope;
        brief.policy = policy_ref.clone();
        let mut memories = Vec::with_capacity(members.len());
        for (memory_id, revision, label) in members {
            memories.push(memory_domain::contracts::MemoryRef {
                memory_id: *memory_id,
                revision: NonZeroU32::new(*revision)
                    .ok_or(Error::Invalid("Memory revision must be positive".into()))?,
                label: label.clone(),
            });
        }
        brief.inputs = WorkInputs {
            sources: vec![],
            memories,
            artifacts: vec![],
        };
        brief.capabilities.sources.clear();
        brief.known_conflicts = vec![];
        brief.evidence_cutoff = Utc::now();
        brief.output_criteria = vec![
            "Compare the cohort and synthesize a conditional clause or candidate procedure that preserves exceptions"
                .into(),
        ];
        let job = self
            .submit_scheduled(
                authority,
                cohort_id,
                brief,
                deadline,
                origin.spec.retain_until.max(deadline + Duration::days(1)),
            )
            .await?;
        Ok(job.id)
    }
}
