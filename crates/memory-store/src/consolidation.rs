use crate::{Error, Result, Store, coordinator::fence};
use memory_domain::{consolidation::*, contracts::*, coordination::Fence, records::*};
use sqlx::Row;
use std::collections::{BTreeSet, VecDeque};
use uuid::Uuid;

impl Store {
    pub async fn consolidation_window(
        &self,
        auth: &Authority,
        permit: &Fence,
        id: Uuid,
        selection: CohortSelection,
    ) -> Result<ConsolidationWindow> {
        let job = self.assigned_job(auth, permit).await?;
        if job.cancel_requested || job.spec.brief.process != Process::Consolidation {
            return Err(Error::Forbidden);
        }
        if selection.members.is_empty()
            || selection.members.len() > 16
            || selection.purpose.trim().is_empty()
            || selection.mechanism.trim().is_empty()
            || selection.source_context.trim().is_empty()
        {
            return Err(Error::Invalid(
                "Select 1 to 16 cases with purpose, mechanism and source context".into(),
            ));
        }
        let auth = Authority {
            scope: job.spec.brief.scope.clone(),
            ..auth.clone()
        };
        if selection
            .members
            .iter()
            .any(|r| !job.spec.brief.inputs.memories.contains(r))
        {
            return Err(Error::Forbidden);
        }
        if let Some(row) = sqlx::query(
            "SELECT data FROM consolidation_windows WHERE tenant_id=$1 AND id=$2 AND job_id=$3",
        )
        .bind(auth.tenant_id.to_string())
        .bind(id.to_string())
        .bind(job.id.to_string())
        .fetch_optional(&self.pool)
        .await?
        {
            let prior: ConsolidationWindow = serde_json::from_str(row.get("data"))?;
            if serde_json::to_value(&prior.selection)? != serde_json::to_value(&selection)? {
                return Err(Error::Conflict);
            }
            self.check_cohort(&auth, &prior).await?;
            return Ok(prior);
        }
        let mut window = ConsolidationWindow {
            id,
            job_id: job.id,
            scope: auth.scope.clone(),
            policy: job.spec.brief.policy.clone(),
            cutoff: job.spec.brief.evidence_cutoff,
            selection,
            cases: vec![],
            source_groups: vec![],
            unknown_lineage: vec![],
            exceptions: vec![],
            unexamined: vec![],
        };
        let mut pending: VecDeque<_> = window.selection.members.clone().into();
        while let Some(reference) = pending.pop_front() {
            if window.cases.iter().any(|c| c.reference == reference) {
                continue;
            }
            if window.cases.len() >= 16 {
                window
                    .unexamined
                    .push("Cohort/exception bound reached".into());
                break;
            }
            let record = self.current_consolidation_memory(&auth, &reference).await?;
            if record.recorded.recorded_at > window.cutoff {
                return Err(Error::Invalid(
                    "Cohort input is newer than its evidence cutoff".into(),
                ));
            }
            let relations = self
                .relations(
                    &auth,
                    reference.memory_id,
                    &MemoryQuery {
                        limit: 65,
                        ..Default::default()
                    },
                )
                .await?;
            if relations.len() > 64 {
                window.unexamined.push("Relation bound reached".into());
            }
            for r in relations.iter().take(64) {
                if matches!(
                    r.relation.kind,
                    RelationKind::Challenges | RelationKind::ConflictsWith
                ) {
                    let other = if r.relation.kind == RelationKind::Challenges {
                        &r.relation.from
                    } else if r.relation.from.memory_id == reference.memory_id {
                        &r.relation.to
                    } else {
                        &r.relation.from
                    };
                    if !window.exceptions.contains(other) {
                        window.exceptions.push(other.clone());
                    }
                    pending.push_back(other.clone());
                }
            }
            if let MemoryContent::Procedure {
                counterexamples, ..
            } = &record.record.content
            {
                for other in counterexamples {
                    if !window.exceptions.contains(other) {
                        window.exceptions.push(other.clone());
                    }
                    pending.push_back(other.clone());
                }
            }
            // Source versions and restatements of one source are one evidence group.
            let mut roots = BTreeSet::new();
            let mut ancestry = vec![record.clone()];
            let mut seen = BTreeSet::new();
            let mut known = true;
            while let Some(input) = ancestry.pop() {
                if !seen.insert(input.version_id) {
                    continue;
                }
                if seen.len() > 64 {
                    known = false;
                    break;
                }
                if input.record.source_locators.is_empty() && input.record.derived_from.is_empty() {
                    known = false;
                }
                for locator in &input.record.source_locators {
                    roots.insert(self.source_locator(&auth, *locator).await?.source.source_id);
                }
                for parent in &input.record.derived_from {
                    ancestry.push(self.memory(&auth, parent).await?);
                }
            }
            if !known || roots.is_empty() {
                window.unknown_lineage.push(reference.clone());
            }
            if !roots.is_empty() {
                let mut group = SourceGroup {
                    sources: roots.into_iter().collect(),
                    members: vec![reference],
                };
                let mut i = 0;
                while i < window.source_groups.len() {
                    if window.source_groups[i]
                        .sources
                        .iter()
                        .any(|s| group.sources.contains(s))
                    {
                        let prior = window.source_groups.remove(i);
                        group.sources.extend(prior.sources);
                        group.members.extend(prior.members);
                        i = 0;
                    } else {
                        i += 1;
                    }
                }
                group.sources.sort();
                group.sources.dedup();
                window.source_groups.push(group);
            }
            window.cases.push(record);
        }
        self.check_cohort(&auth, &window).await?;
        sqlx::query(
            "INSERT INTO consolidation_windows(tenant_id,id,job_id,data) VALUES($1,$2,$3,$4)",
        )
        .bind(auth.tenant_id.to_string())
        .bind(id.to_string())
        .bind(job.id.to_string())
        .bind(serde_json::to_string(&window)?)
        .execute(&self.pool)
        .await?;
        Ok(window)
    }
    pub async fn current_consolidation_memory(
        &self,
        auth: &Authority,
        reference: &MemoryRef,
    ) -> Result<MemoryVersion> {
        let record = self.memory(auth, reference).await?;
        if record.recorded_until.is_some() || record.record.availability != Availability::Routine {
            return Err(Error::Unavailable);
        }
        Ok(record)
    }
    pub async fn check_cohort(&self, auth: &Authority, window: &ConsolidationWindow) -> Result<()> {
        for case in &window.cases {
            self.current_consolidation_memory(auth, &case.reference)
                .await?;
        }
        Ok(())
    }
    pub async fn save_consolidation_review(
        &self,
        auth: &Authority,
        permit: &Fence,
        review: &ConsolidationReview,
    ) -> Result<()> {
        let (mut tx, at) = self.begin_write().await?;
        let job = fence(&mut tx, auth, permit, at.recorded_at).await?;
        if job.cancel_requested || job.id != review.window.job_id {
            return Err(Error::Forbidden);
        }
        let data = serde_json::to_string(review)?;
        if let Some(row) =
            sqlx::query("SELECT data FROM consolidation_reviews WHERE tenant_id=$1 AND id=$2")
                .bind(auth.tenant_id.to_string())
                .bind(review.id.to_string())
                .fetch_optional(&mut *tx)
                .await?
        {
            if serde_json::from_str::<serde_json::Value>(row.get("data"))?
                != serde_json::to_value(review)?
            {
                return Err(Error::Conflict);
            }
        } else {
            sqlx::query(
                "INSERT INTO consolidation_reviews(tenant_id,id,job_id,data) VALUES($1,$2,$3,$4)",
            )
            .bind(auth.tenant_id.to_string())
            .bind(review.id.to_string())
            .bind(job.id.to_string())
            .bind(data)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }
    pub async fn consolidation_review(
        &self,
        auth: &Authority,
        permit: &Fence,
        id: Uuid,
    ) -> Result<ConsolidationReview> {
        self.assigned_job(auth, permit).await?;
        let row = sqlx::query(
            "SELECT data FROM consolidation_reviews WHERE tenant_id=$1 AND id=$2 AND job_id=$3",
        )
        .bind(auth.tenant_id.to_string())
        .bind(id.to_string())
        .bind(permit.job_id.to_string())
        .fetch_one(&self.pool)
        .await?;
        let review: ConsolidationReview = serde_json::from_str(row.get("data"))?;
        self.check_cohort(auth, &review.window).await?;
        Ok(review)
    }
    /// The host supplies the disposition after checking the semantic decisions.
    pub async fn commit_consolidation(
        &self,
        auth: &Authority,
        permit: &Fence,
        review: &ConsolidationReview,
        mut result: ConsolidationResult,
    ) -> Result<ConsolidationResult> {
        let (mut tx, at) = self.begin_write().await?;
        let job = fence(&mut tx, auth, permit, at.recorded_at).await?;
        if job.cancel_requested || job.spec.brief.policy != review.window.policy {
            return Err(Error::Forbidden);
        }
        crate::policies::current_policy(&mut tx, auth, &review.window.policy).await?;
        for input in &review.window.cases {
            let current = crate::memories::memory_version(&mut tx, auth, &input.reference).await?;
            if current.recorded_until.is_some()
                || current.record.availability != Availability::Routine
            {
                return Err(Error::Conflict);
            }
        }
        let row = sqlx::query(
            "SELECT result FROM consolidation_reviews WHERE tenant_id=$1 AND id=$2 AND job_id=$3",
        )
        .bind(auth.tenant_id.to_string())
        .bind(review.id.to_string())
        .bind(job.id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if let Some(data) = row.get::<Option<String>, _>("result") {
            let result: ConsolidationResult = serde_json::from_str(&data)?;
            if let Some(record) = &result.record {
                crate::memories::memory_version(&mut tx, auth, &record.reference).await?;
            }
            return Ok(result);
        }
        if result.deferred.is_empty() {
            let p = &review.proposal;
            let content = p.procedure.clone().unwrap_or(MemoryContent::Knowledge {
                statement: p
                    .clauses
                    .iter()
                    .map(|c| {
                        format!(
                            "{} Conditions: {}. Uncertainty: {}.",
                            c.text,
                            c.conditions.join("; "),
                            c.uncertainty.join("; ")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                subject: None,
                predicate: None,
                uncertainty: p.untested.clone(),
                examined_coverage: vec![review.window.selection.purpose.clone()],
            });
            let simulation = review
                .window
                .cases
                .iter()
                .any(|c| c.record.evidential_status == EvidentialStatus::Simulation);
            let draft = RecordDraft { label: p.label.clone(), scope: p.scope.clone(), content, origin: Origin::AgentGenerated,
                evidential_status: if simulation { EvidentialStatus::Simulation } else { EvidentialStatus::Inference }, availability: Availability::Routine,
                qualification: Qualification::Candidate, valid_time: ValidTime::Unknown, source_locators: vec![],
                derived_from: review.window.cases.iter().map(|c| c.reference.clone()).collect(),
                decision: PolicyDecision { policy: review.window.policy.clone(), action: PolicyAction::Retain, reason: "Supported conditional synthesis; procedural use still requires qualification".into(), constraints: p.untested.clone(), required_evidence: vec![format!("Consolidation review {}", review.id)], expires_at: None } };
            let record = crate::memories::apply_changes(
                &mut tx,
                auth,
                vec![MemoryChange::Create(draft)],
                at,
            )
            .await?
            .remove(0);
            for exception in &review.window.exceptions {
                crate::relations::insert_relation(
                    &mut tx,
                    auth,
                    RelationDraft {
                        from: exception.clone(),
                        to: record.reference.clone(),
                        kind: RelationKind::Challenges,
                        scope: p.scope.clone(),
                        basis: "Preserved cohort exception".into(),
                        evidential_status: EvidentialStatus::Inference,
                        acceptance: Acceptance::Accepted,
                        valid_time: ValidTime::Unknown,
                    },
                    at,
                )
                .await?;
            }
            result.record = Some(record);
        }
        sqlx::query("UPDATE consolidation_reviews SET result=$1 WHERE tenant_id=$2 AND id=$3")
            .bind(serde_json::to_string(&result)?)
            .bind(auth.tenant_id.to_string())
            .bind(review.id.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(result)
    }
}
impl Store {
    pub async fn consolidation_result(
        &self,
        auth: &Authority,
        permit: &Fence,
        id: Uuid,
    ) -> Result<ConsolidationResult> {
        self.consolidation_review(auth, permit, id).await?;
        let row = sqlx::query(
            "SELECT result FROM consolidation_reviews WHERE tenant_id=$1 AND id=$2 AND job_id=$3",
        )
        .bind(auth.tenant_id.to_string())
        .bind(id.to_string())
        .bind(permit.job_id.to_string())
        .fetch_one(&self.pool)
        .await?;
        serde_json::from_str(
            &row.get::<Option<String>, _>("result")
                .ok_or(Error::Unavailable)?,
        )
        .map_err(Error::from)
    }
    pub async fn adoption(
        &self,
        auth: &Authority,
        evaluation_job: Uuid,
    ) -> Result<Option<AdoptionResult>> {
        self.job(auth, evaluation_job).await?;
        let row = sqlx::query(
            "SELECT data FROM qualification_adoptions WHERE tenant_id=$1 AND job_id=$2",
        )
        .bind(auth.tenant_id.to_string())
        .bind(evaluation_job.to_string())
        .fetch_optional(&self.pool)
        .await?;
        match row {
            None => Ok(None),
            Some(row) => {
                let result: AdoptionResult = serde_json::from_str(row.get("data"))?;
                self.current_consolidation_memory(auth, &result.candidate.reference)
                    .await?;
                Ok(Some(result))
            }
        }
    }
    pub async fn adopt_procedure(
        &self,
        auth: &Authority,
        permit: &Fence,
        input: &QualificationInput,
        report_artifact: Uuid,
        conditions: Vec<String>,
        mut result: AdoptionResult,
    ) -> Result<AdoptionResult> {
        let (mut tx, at) = self.begin_write().await?;
        let parent = fence(&mut tx, auth, permit, at.recorded_at).await?;
        if parent.cancel_requested || parent.id != input.review.window.job_id {
            return Err(Error::Forbidden);
        }
        crate::policies::current_policy(&mut tx, auth, &input.review.window.policy).await?;
        crate::artifacts::ready_artifact(&mut tx, auth, report_artifact).await?;
        let child = crate::coordinator::job(&mut tx, auth, result.evaluation_job).await?;
        if child.state != memory_domain::coordination::JobState::Completed
            || child.spec.parent_id != Some(parent.id)
        {
            return Err(Error::Forbidden);
        }
        for case in input
            .review
            .window
            .cases
            .iter()
            .chain(std::iter::once(&input.candidate))
        {
            let current = crate::memories::memory_version(&mut tx, auth, &case.reference).await?;
            if current.recorded_until.is_some()
                || current.record.availability != Availability::Routine
            {
                return Err(Error::Conflict);
            }
        }
        if result.adopted {
            let mut record = input.candidate.record.clone();
            if let MemoryContent::Procedure { applicability, .. } = &mut record.content {
                applicability.push(format!(
                    "Task fits a tested context: ({})",
                    conditions.join(") OR (")
                ));
            }
            record.qualification = Qualification::Evaluated {
                conditions,
                evidence: record.derived_from.clone(),
                evaluation_artifact: Some(report_artifact),
                evaluator_profile: Some(child.spec.brief.profile.clone()),
            };
            record.decision.action = PolicyAction::Qualify;
            record.decision.reason =
                "Held-out comparison meets the configured adoption policy".into();
            result.candidate = crate::memories::apply_changes(
                &mut tx,
                auth,
                vec![MemoryChange::Revise {
                    expected: input.candidate.reference.clone(),
                    record,
                }],
                at,
            )
            .await?
            .remove(0);
        }
        sqlx::query("INSERT INTO qualification_adoptions(tenant_id,job_id,data) VALUES($1,$2,$3)")
            .bind(auth.tenant_id.to_string())
            .bind(result.evaluation_job.to_string())
            .bind(serde_json::to_string(&result)?)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(result)
    }
}
impl Store {
    pub async fn qualification_evidence(
        &self,
        auth: &Authority,
        job_id: Uuid,
        artifact_id: Uuid,
    ) -> Result<()> {
        let mut c = self.pool.acquire().await?;
        let owned = sqlx::query("SELECT artifact_id FROM job_artifacts WHERE tenant_id=$1 AND job_id=$2 AND artifact_id=$3").bind(auth.tenant_id.to_string()).bind(job_id.to_string()).bind(artifact_id.to_string()).fetch_optional(&mut *c).await?.is_some();
        if !owned {
            return Err(Error::Forbidden);
        }
        crate::artifacts::ready_artifact(&mut c, auth, artifact_id).await?;
        Ok(())
    }
}
