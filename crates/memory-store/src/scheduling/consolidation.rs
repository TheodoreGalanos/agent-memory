//! Consolidation cohort selection and job planning.
use super::*;
use memory_domain::{
    consolidation::MAX_COHORT_SIZE,
    contracts::{Process, WorkInputs},
    records::{Qualification, RecordDraft},
};
use std::num::NonZeroU32;
const CANDIDATE_SCAN: i64 = 500;

impl Store {
    /// Groups retained, not-yet-consolidated candidate records by policy and, once a policy's
    /// group reaches its configured minimum, submits one bounded consolidation job over the
    /// oldest members and records them as covered so a repeated sweep does not reform the cohort.
    /// A later sweep may still form a new cohort from records recorded since.
    pub async fn schedule_consolidation(&self, authority: &Authority, limit: u16) -> Result<u32> {
        let limit = limit.clamp(1, 100);
        let (filter, values) = predicate(authority);
        let after_sequence = values.len() + 1;
        let after_id = values.len() + 2;
        let sql = format!(
            "SELECT mv.id,mv.revision,mv.data,mv.recorded_from FROM memory_versions mv \
             JOIN resource_scopes s ON s.tenant_id=mv.tenant_id AND s.id=mv.id \
             WHERE {filter} AND mv.recorded_to IS NULL AND mv.availability='routine' \
             AND NOT EXISTS (SELECT 1 FROM consolidation_cohort_members m WHERE m.tenant_id=mv.tenant_id AND m.memory_id=mv.id) \
             AND (mv.recorded_from>${after_sequence} OR (mv.recorded_from=${after_sequence} AND mv.id>${after_id})) \
             ORDER BY mv.recorded_from,mv.id LIMIT {CANDIDATE_SCAN}"
        );
        let mut groups: std::collections::BTreeMap<(Uuid, u32), Vec<CohortMember>> =
            std::collections::BTreeMap::new();
        let mut after = (0i64, String::new());
        loop {
            let mut query = sqlx::query(&sql);
            for v in &values {
                query = query.bind(v);
            }
            let rows = query
                .bind(after.0)
                .bind(&after.1)
                .fetch_all(&self.pool)
                .await?;
            if rows.is_empty() {
                break;
            }
            for row in rows {
                after = (row.try_get("recorded_from")?, row.try_get("id")?);
                let memory_id = id(row.try_get("id")?)?;
                let revision: u32 = crate::database::number(row.try_get("revision")?)?;
                let data: String = row.try_get("data")?;
                let record: RecordDraft = serde_json::from_str(&data)?;
                if !matches!(record.qualification, Qualification::Candidate) {
                    continue;
                }
                let members = groups
                    .entry((
                        record.decision.policy.id,
                        record.decision.policy.revision.get(),
                    ))
                    .or_default();
                // Keep only the oldest members that can fit in any valid cohort.
                if members.len() < usize::from(MAX_COHORT_SIZE) {
                    members.push((memory_id, revision, record.label));
                }
            }
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
            let plan = self
                .schedule_one_consolidation(
                    authority,
                    cohort_id,
                    &policy_ref,
                    &policy_scope,
                    &members,
                )
                .await;
            scheduled += self
                .commit_schedule(
                    authority,
                    ScheduleTrigger {
                        kind: "consolidation",
                        id: cohort_id,
                        dedupe_key,
                        causal: None,
                    },
                    plan,
                    Some((&policy_ref, &members)),
                )
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
    ) -> Result<Command<SubmitJob>> {
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
        Ok(Self::scheduled_command(
            authority,
            cohort_id,
            brief,
            deadline,
            origin.spec.retain_until.max(deadline + Duration::days(1)),
        ))
    }
}
