//! Qualification triggers and job planning.
use super::*;
use memory_domain::{
    contracts::{Process, WorkInputs},
    records::{MemoryContent, Qualification},
};

impl Store {
    /// Schedules one qualification/evaluation job for each consolidation commit that produced a
    /// candidate procedure, once. This is a best-effort, unattended trial: it runs a real held-out
    /// comparison only when the originating consolidation job already carried a suite artifact, and
    /// reports a clear partial result otherwise. Adoption stays a deliberate action on the existing
    /// `AdoptProcedure` check, which needs a live fence on the still-open job that produced the
    /// candidate; a session that wants adoption drives `start_qualification`/`adopt_procedure`
    /// itself on that fence, exactly as before this scheduler existed.
    pub async fn schedule_qualification(&self, authority: &Authority, limit: u16) -> Result<u32> {
        let limit = limit.clamp(1, 100);
        let mut scheduled = 0u32;
        let mut after = String::new();
        while scheduled < u32::from(limit) {
            let rows = sqlx::query(
            "SELECT r.id,r.job_id,r.result FROM consolidation_reviews r WHERE r.tenant_id=$1 AND r.result IS NOT NULL AND r.id>$2 \
             AND NOT EXISTS (SELECT 1 FROM scheduled_work w WHERE w.tenant_id=r.tenant_id AND w.trigger_kind='qualification' AND w.dedupe_key=r.id) \
             ORDER BY r.id LIMIT 500",
        )
        .bind(authority.tenant_id.to_string())
        .bind(&after)
        .fetch_all(&self.pool)
        .await?;
            if rows.is_empty() {
                break;
            }
            for row in rows {
                after = row.try_get("id")?;
                if scheduled >= u32::from(limit) {
                    break;
                }
                let review_id: Uuid = id(row.try_get("id")?)?;
                let job_id: Uuid = id(row.try_get("job_id")?)?;
                let result: memory_domain::consolidation::ConsolidationResult =
                    serde_json::from_str(row.try_get("result")?)?;
                let Some(candidate) = result.record else {
                    continue;
                };
                if !matches!(candidate.record.content, MemoryContent::Procedure { .. })
                    || !matches!(candidate.record.qualification, Qualification::Candidate)
                {
                    continue;
                }
                let trigger_id = candidate.reference.memory_id;
                if self
                    .scheduled_work(authority, "qualification", trigger_id)
                    .await?
                    .is_some()
                {
                    continue;
                }
                let mut connection = self.pool.acquire().await?;
                if crate::access::read_scope(&mut connection, authority, trigger_id)
                    .await
                    .is_err()
                {
                    continue; // Another authority's scope; its own timer schedules it.
                }
                drop(connection);
                let plan = self
                    .schedule_one_qualification(authority, review_id, job_id, &candidate)
                    .await;
                scheduled += self
                    .commit_schedule(
                        authority,
                        ScheduleTrigger {
                            kind: "qualification",
                            id: trigger_id,
                            dedupe_key: review_id.to_string(),
                            causal: Some(job_id),
                        },
                        plan,
                        None,
                    )
                    .await?;
            }
        }
        Ok(scheduled)
    }

    async fn schedule_one_qualification(
        &self,
        authority: &Authority,
        review_id: Uuid,
        consolidation_job_id: Uuid,
        candidate: &memory_domain::records::MemoryVersion,
    ) -> Result<Command<SubmitJob>> {
        let origin = self.job(authority, consolidation_job_id).await?;
        let deadline = self
            .scheduled_deadline(authority, origin.spec.brief.limits.root_budget_id)
            .await?;
        let (task_id, scope) = scoped_task(&candidate.record.scope);
        let mut brief = origin.spec.brief.clone();
        // The evaluation pool module (not consolidation's) runs a qualification job; it reads the
        // candidate from `inputs.memories[0]` and, when the originating consolidation job carried a
        // held-out suite artifact, forwards it unchanged so a real comparison can run. Without one it
        // reports a clear partial result rather than fabricating held-out cases (see evaluation.ts).
        brief.process = Process::Evaluation;
        brief.purpose = format!("qualify procedure {}", candidate.reference.label);
        brief.task_id = task_id;
        brief.scope = scope;
        brief.inputs = WorkInputs {
            sources: vec![],
            memories: vec![candidate.reference.clone()],
            artifacts: origin.spec.brief.inputs.artifacts.clone(),
        };
        brief.known_conflicts = vec![];
        brief.evidence_cutoff = Utc::now();
        brief.output_criteria = vec![
            "Build a held-out evaluation suite and qualify the candidate procedure by comparison against episode and summary baselines"
                .into(),
        ];
        Ok(Self::scheduled_command(
            authority,
            review_id,
            brief,
            deadline,
            origin.spec.retain_until.max(deadline + Duration::days(1)),
        ))
    }
}
