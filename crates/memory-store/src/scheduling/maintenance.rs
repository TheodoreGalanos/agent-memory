//! Correction and source revision triggers and maintenance job planning.
use super::*;
use memory_domain::{
    contracts::{Capabilities, Process, ScheduledInputs, WorkInputs},
    records::{RecordDraft, RelationKind},
};
use std::num::NonZeroU32;
/// Which kind of change a scheduled maintenance job propagates, matching the `ScheduledInputs`
/// variant its brief carries, together with the reason for that change.
enum MaintenanceTrigger {
    Correction(String),
    Source(String),
}

impl Store {
    /// Schedules one maintenance job per new source revision or memory correction, over the
    /// affected memory and its dependents. Skips a trigger whose causal origin is itself a
    /// maintenance job over the same records, so a module never reprocesses its own output.
    pub async fn schedule_maintenance(&self, authority: &Authority, limit: u16) -> Result<u32> {
        let limit = limit.clamp(1, 100);
        let mut scheduled = 0u32;
        scheduled += self
            .schedule_maintenance_corrections(authority, limit.saturating_sub(scheduled as u16))
            .await?;
        if scheduled < u32::from(limit) {
            scheduled += self
                .schedule_maintenance_sources(authority, limit - scheduled as u16)
                .await?;
        }
        Ok(scheduled)
    }

    async fn schedule_maintenance_corrections(
        &self,
        authority: &Authority,
        limit: u16,
    ) -> Result<u32> {
        let mut scheduled = 0u32;
        let mut after = 0i64;
        while scheduled < u32::from(limit) {
            let rows = sqlx::query(
            "SELECT m.id,m.cursor,m.resource_id,m.data,m.causal_job_id FROM memory_changes m WHERE m.tenant_id=$1 AND m.cursor>$2 \
             AND NOT EXISTS (SELECT 1 FROM scheduled_work w WHERE w.tenant_id=m.tenant_id AND w.trigger_kind='maintenance-correction' AND w.trigger_id=m.id) \
             ORDER BY m.cursor LIMIT 500",
        )
        .bind(authority.tenant_id.to_string())
        .bind(after)
        .fetch_all(&self.pool)
        .await?;
            if rows.is_empty() {
                break;
            }
            for row in rows {
                after = row.try_get("cursor")?;
                if scheduled >= u32::from(limit) {
                    break;
                }
                let notice_id: Uuid = id(row.try_get("id")?)?;
                if self
                    .scheduled_work(authority, "maintenance-correction", notice_id)
                    .await?
                    .is_some()
                {
                    continue;
                }
                let notice: memory_domain::maintenance::MemoryChangeNotice =
                    serde_json::from_str(row.try_get("data")?)?;
                if notice.kind != memory_domain::maintenance::ChangeKind::Correction {
                    continue;
                }
                let mut connection = self.pool.acquire().await?;
                if crate::access::read_scope(&mut connection, authority, notice.current.memory_id)
                    .await
                    .is_err()
                {
                    continue;
                }
                drop(connection);
                let causal_job_id: Option<String> = row.try_get("causal_job_id")?;
                let causal_job_id = causal_job_id.map(|v| id(&v)).transpose()?;
                if let Some(causal) = causal_job_id {
                    if let Ok(job) = self.job(authority, causal).await {
                        if job.spec.brief.process == Process::Maintenance {
                            self.record_unschedulable(
                                authority,
                                "maintenance-correction",
                                notice_id,
                                &notice.current.memory_id.to_string(),
                                "causal origin is a maintenance job over these records",
                                Some(causal),
                            )
                            .await?;
                            continue;
                        }
                    }
                }
                let current = self
                    .current_memory(authority, notice.current.memory_id)
                    .await;
                let current = match current {
                    Ok(m) => m,
                    Err(Error::NotFound | Error::Unavailable) => continue,
                    Err(e) => return Err(e),
                };
                let mut members = vec![current.reference.clone()];
                let dependents = self
                    .relations(
                        authority,
                        notice.current.memory_id,
                        &memory_domain::records::MemoryQuery {
                            limit: 65,
                            ..Default::default()
                        },
                    )
                    .await?;
                for edge in dependents {
                    if edge.relation.kind == RelationKind::DependsOn
                        && edge.relation.to.memory_id == notice.current.memory_id
                        && !members.contains(&edge.relation.from)
                    {
                        members.push(edge.relation.from.clone());
                    }
                    if members.len() >= 16 {
                        break;
                    }
                }
                let dedupe_key = members
                    .iter()
                    .map(|m| m.memory_id.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                let plan = self
                    .schedule_one_maintenance(
                        authority,
                        notice_id,
                        &current.record.decision.policy,
                        &members,
                        &current.record,
                        MaintenanceTrigger::Correction(notice.reason.clone()),
                    )
                    .await;
                scheduled += self
                    .commit_schedule(
                        authority,
                        ScheduleTrigger {
                            kind: "maintenance-correction",
                            id: notice_id,
                            dedupe_key,
                            causal: causal_job_id,
                        },
                        plan,
                        None,
                    )
                    .await?;
            }
        }
        Ok(scheduled)
    }

    async fn schedule_maintenance_sources(&self, authority: &Authority, limit: u16) -> Result<u32> {
        let mut scheduled = 0u32;
        let mut after = 0i64;
        while scheduled < u32::from(limit) {
            let rows = sqlx::query(
            "SELECT e.id,e.resource_id,e.cursor FROM outbox_events e WHERE e.tenant_id=$1 AND e.kind='source_registered' AND e.cursor>$2 \
             AND NOT EXISTS (SELECT 1 FROM scheduled_work w WHERE w.tenant_id=e.tenant_id AND w.trigger_kind='maintenance-source' AND w.trigger_id=e.id) \
             ORDER BY e.cursor LIMIT 500",
        )
        .bind(authority.tenant_id.to_string())
        .bind(after)
        .fetch_all(&self.pool)
        .await?;
            if rows.is_empty() {
                break;
            }
            for row in rows {
                after = row.try_get("cursor")?;
                if scheduled >= u32::from(limit) {
                    break;
                }
                let event_id: Uuid = id(row.try_get("id")?)?;
                if self
                    .scheduled_work(authority, "maintenance-source", event_id)
                    .await?
                    .is_some()
                {
                    continue;
                }
                let scope_id: Uuid = id(row.try_get("resource_id")?)?;
                let cursor: i64 = row.try_get("cursor")?;
                let source_row = sqlx::query(
                    "SELECT source_id FROM source_versions WHERE tenant_id=$1 AND scope_id=$2",
                )
                .bind(authority.tenant_id.to_string())
                .bind(scope_id.to_string())
                .fetch_optional(&self.pool)
                .await?;
                let Some(source_row) = source_row else {
                    continue;
                };
                let source_id: Uuid = id(source_row.try_get("source_id")?)?;
                // A source's own first registration is not yet a "new" revision to react to; only
                // an earlier `source_registered` event for the same source makes this one a revision.
                let earlier: i64 = sqlx::query(
                "SELECT COUNT(*) AS n FROM outbox_events e2 \
                 JOIN source_versions sv2 ON sv2.tenant_id=e2.tenant_id AND sv2.scope_id=e2.resource_id \
                 WHERE e2.tenant_id=$1 AND e2.kind='source_registered' AND sv2.source_id=$2 AND e2.cursor<$3",
            )
            .bind(authority.tenant_id.to_string())
            .bind(source_id.to_string())
            .bind(cursor)
            .fetch_one(&self.pool)
            .await?
            .try_get("n")?;
                if earlier == 0 {
                    self.record_unschedulable(
                        authority,
                        "maintenance-source",
                        event_id,
                        &source_id.to_string(),
                        "Initial source registration, not a revision",
                        None,
                    )
                    .await?;
                    continue;
                }
                let member_rows = sqlx::query(
                "SELECT DISTINCT mv.id,mv.revision,mv.data FROM memory_sources ms \
                 JOIN source_locators sl ON sl.tenant_id=ms.tenant_id AND sl.id=ms.locator_id \
                 JOIN memory_versions mv ON mv.tenant_id=ms.tenant_id AND mv.id=ms.memory_id AND mv.revision=ms.revision \
                 WHERE ms.tenant_id=$1 AND sl.source_id=$2 AND mv.recorded_to IS NULL LIMIT 16",
            )
            .bind(authority.tenant_id.to_string())
            .bind(source_id.to_string())
            .fetch_all(&self.pool)
            .await?;
                if member_rows.is_empty() {
                    self.record_unschedulable(
                        authority,
                        "maintenance-source",
                        event_id,
                        &source_id.to_string(),
                        "No retained record depends on this source",
                        None,
                    )
                    .await?;
                    continue;
                }
                let mut members = vec![];
                let mut policy = None;
                let mut before_record = None;
                let mut readable = true;
                for row in &member_rows {
                    let memory_id = id(row.try_get("id")?)?;
                    let revision: u32 = crate::database::number(row.try_get("revision")?)?;
                    let data: String = row.try_get("data")?;
                    let record: RecordDraft = serde_json::from_str(&data)?;
                    let mut connection = self.pool.acquire().await?;
                    if crate::access::read_scope(&mut connection, authority, memory_id)
                        .await
                        .is_err()
                    {
                        readable = false;
                        break;
                    }
                    drop(connection);
                    policy.get_or_insert_with(|| record.decision.policy.clone());
                    before_record.get_or_insert_with(|| record.clone());
                    members.push(memory_domain::contracts::MemoryRef {
                        memory_id,
                        revision: NonZeroU32::new(revision)
                            .ok_or(Error::Invalid("Memory revision must be positive".into()))?,
                        label: record.label,
                    });
                }
                if !readable {
                    continue; // Another authority's scope; its own timer schedules it.
                }
                let dedupe_key = members
                    .iter()
                    .map(|m| m.memory_id.to_string())
                    .collect::<Vec<_>>()
                    .join(",");
                let Some(policy) = policy else { continue };
                let Some(before_record) = before_record else {
                    continue;
                };
                let plan = self
                    .schedule_one_maintenance(
                        authority,
                        event_id,
                        &policy,
                        &members,
                        &before_record,
                        MaintenanceTrigger::Source(format!("Source {source_id} was revised")),
                    )
                    .await;
                scheduled += self
                    .commit_schedule(
                        authority,
                        ScheduleTrigger {
                            kind: "maintenance-source",
                            id: event_id,
                            dedupe_key,
                            causal: None,
                        },
                        plan,
                        None,
                    )
                    .await?;
            }
        }
        Ok(scheduled)
    }

    async fn schedule_one_maintenance(
        &self,
        authority: &Authority,
        trigger_id: Uuid,
        policy: &ConfigRef,
        members: &[memory_domain::contracts::MemoryRef],
        before_record: &RecordDraft,
        trigger: MaintenanceTrigger,
    ) -> Result<Command<SubmitJob>> {
        // Validates the policy exists and is current; scoped_task below uses the affected record's own
        // scope instead of the policy's: a project-wide policy's consolidation cohort can invent a
        // fresh task for its new candidate, but maintenance revises an *existing* record, whose own
        // scope a freshly-invented task would not permit writing to.
        self.policy(authority, policy).await?;
        let (task_id, scope) = scoped_task(&before_record.scope);
        let origin = self.recent_contributing_job(authority, &scope).await?;
        let deadline = self
            .scheduled_deadline(authority, origin.spec.brief.limits.root_budget_id)
            .await?;
        // The maintenance handler (packages/supervisor/src/maintenance.ts) needs a `MaintenanceRequest`
        // to review; the scheduler builds it directly from the trigger and the already-loaded affected
        // records rather than smuggling it through a string, since it holds no job fence and so cannot
        // publish a manifest artifact the way an interactive session would. A request needs a proposed
        // revision or a removed source (`review_maintenance` rejects a bare revalidation); since the
        // scheduler has no way to author a substantive content change, `after` repeats `before`'s own
        // content with only the decision reason updated — a wording-only revision that still runs J13
        // through J16 review of the affected record and its dependents against the trigger.
        let [before, candidates @ ..] = members else {
            return Err(Error::Invalid(
                "Maintenance needs at least one affected record".into(),
            ));
        };
        let reason = match &trigger {
            MaintenanceTrigger::Correction(reason) | MaintenanceTrigger::Source(reason) => {
                reason.clone()
            }
        };
        let mut after = before_record.clone();
        after.decision.reason = reason.clone();
        let request = memory_domain::maintenance::MaintenanceRequest {
            before: before.clone(),
            after: Some(after),
            removed_sources: vec![],
            candidates: candidates.to_vec(),
            reason,
        };
        let mut brief = origin.spec.brief.clone();
        brief.process = Process::Maintenance;
        brief.purpose = format!("maintain affected records for trigger {trigger_id}");
        brief.task_id = task_id;
        brief.scope = scope;
        brief.policy = policy.clone();
        brief.inputs = WorkInputs {
            sources: vec![],
            memories: members.to_vec(),
            artifacts: vec![],
        };
        brief.capabilities = Capabilities {
            sources: vec![],
            queries: vec![],
            tools: origin.spec.brief.capabilities.tools.clone(),
        };
        brief.known_conflicts = vec![];
        brief.evidence_cutoff = Utc::now();
        brief.output_criteria = vec![
            "Review the affected records against current support and propose a maintained revision or removal"
                .into(),
        ];
        let request = Box::new(request);
        brief.scheduled = Some(Box::new(match trigger {
            MaintenanceTrigger::Correction(_) => ScheduledInputs::MaintenanceCorrection { request },
            MaintenanceTrigger::Source(_) => ScheduledInputs::MaintenanceSource { request },
        }));
        Ok(Self::scheduled_command(
            authority,
            trigger_id,
            brief,
            deadline,
            origin.spec.retain_until.max(deadline + Duration::days(1)),
        ))
    }
}
