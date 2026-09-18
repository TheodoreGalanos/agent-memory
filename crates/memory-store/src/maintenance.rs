use crate::{
    Error, Result, Store,
    coordinator::fence,
    memories::{apply_changes, memory_version},
};
use chrono::Duration;
use memory_domain::{
    activation::ActivationQuery, contracts::*, coordination::Fence, maintenance::*, records::*,
};
use sqlx::Row;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use uuid::Uuid;

pub struct MaintenanceDisposition {
    pub kind: ChangeKind,
    pub comparison: Option<String>,
    pub support: BTreeMap<Uuid, String>,
    pub indirect: BTreeMap<Uuid, String>,
    pub conflicts: BTreeMap<Uuid, String>,
}
impl Store {
    pub async fn maintenance_review(
        &self,
        a: &Authority,
        permit: &Fence,
        id: Uuid,
        request: MaintenanceRequest,
    ) -> Result<MaintenanceReview> {
        let job = self.assigned_job(a, permit).await?;
        if job.cancel_requested
            || job.spec.brief.process != Process::Maintenance
            || !job.spec.brief.inputs.memories.contains(&request.before)
        {
            return Err(Error::Forbidden);
        }
        if request.candidates.len() > 16
            || request.removed_sources.len() > 16
            || request.reason.trim().is_empty()
        {
            return Err(Error::Invalid(
                "Maintenance needs a reason and bounded inputs".into(),
            ));
        }
        let a = Authority {
            scope: job.spec.brief.scope.clone(),
            ..a.clone()
        };
        if let Some(row) = sqlx::query(
            "SELECT data,result FROM maintenance_reviews WHERE tenant_id=$1 AND id=$2 AND job_id=$3",
        )
        .bind(a.tenant_id.to_string())
        .bind(id.to_string())
        .bind(job.id.to_string())
        .fetch_optional(&self.pool)
        .await?
        {
            let r: MaintenanceReview = serde_json::from_str(row.get("data"))?;
            if serde_json::to_value(&r.request)? != serde_json::to_value(&request)? {
                return Err(Error::Conflict);
            }
            if row.get::<Option<String>,_>("result").is_none() {
                self.current_consolidation_memory(&a, &r.before.reference).await?;
            }
            return Ok(r);
        }
        let before = self
            .current_consolidation_memory(&a, &request.before)
            .await?;
        if let Some(after) = &request.after {
            after.validate().map_err(Error::Invalid)?;
            crate::intentions::check_plan(&mut *self.pool.acquire().await?, &a, &job, after)
                .await?;
            if after.scope != before.record.scope
                || after.content.family() != before.record.content.family()
                || after.decision.policy != job.spec.brief.policy
            {
                return Err(Error::Forbidden);
            }
        }
        if request.after.is_none() && request.removed_sources.is_empty() {
            return Err(Error::Invalid(
                "Maintenance requires a proposed revision or removed source".into(),
            ));
        }
        let lost: BTreeSet<_> = request
            .removed_sources
            .iter()
            .map(|s| s.source_id)
            .collect();
        for source in &request.removed_sources {
            let s = self.source_version(&a, source).await?;
            let artifact = s.snapshot_artifact.ok_or(Error::Invalid(
                "Source removal needs an unavailable snapshot".into(),
            ))?;
            let row =
                sqlx::query("SELECT state FROM artifact_records WHERE tenant_id=$1 AND id=$2")
                    .bind(a.tenant_id.to_string())
                    .bind(artifact.to_string())
                    .fetch_optional(&self.pool)
                    .await?
                    .ok_or(Error::Unavailable)?;
            if row.get::<String, _>("state") != "revoked" {
                return Err(Error::Invalid("Source snapshot is still available".into()));
            }
        }
        if !lost.is_empty() {
            let mut ancestry = vec![before.clone()];
            let mut roots = BTreeSet::new();
            let mut seen = BTreeSet::new();
            while let Some(m) = ancestry.pop() {
                if !seen.insert(m.version_id) {
                    continue;
                }
                if seen.len() > 32 {
                    return Err(Error::LimitExceeded);
                }
                for id in &m.record.source_locators {
                    roots.insert(self.source_locator(&a, *id).await?.source.source_id);
                }
                for parent in &m.record.derived_from {
                    ancestry.push(self.memory(&a, parent).await?);
                }
            }
            if !lost.is_subset(&roots) {
                return Err(Error::Invalid(
                    "Removed sources must belong to the reviewed record's lineage".into(),
                ));
            }
        }
        let mut review = MaintenanceReview {
            id,
            job_id: job.id,
            request,
            before: before.clone(),
            affected: vec![],
            indirect: vec![],
            conflicts: vec![],
            relations: vec![],
            coverage: Coverage {
                examined: vec![],
                unexamined: vec![],
            },
        };
        let mut queue = VecDeque::from([before.reference.clone()]);
        let mut seen = BTreeSet::new();
        while let Some(r) = queue.pop_front() {
            if !seen.insert(r.memory_id) {
                continue;
            }
            if review.affected.len() >= 16 {
                review
                    .coverage
                    .unexamined
                    .push("Direct dependency bound reached".into());
                break;
            }
            let claim = self.current_memory(&a, r.memory_id).await?;
            if claim.record.availability != Availability::Routine {
                continue;
            }
            let relations = self
                .relations(
                    &a,
                    r.memory_id,
                    &MemoryQuery {
                        limit: 65,
                        ..Default::default()
                    },
                )
                .await?;
            if relations.len() > 64 {
                review
                    .coverage
                    .unexamined
                    .push("Relation bound reached".into());
            }
            let mut remaining = vec![];
            let mut challenges = vec![];
            let mut governed = claim.record.derived_from.iter().any(|r| {
                r.memory_id == before.reference.memory_id
                    || review.affected.iter().any(|s| {
                        s.governed_derivative && s.claim.reference.memory_id == r.memory_id
                    })
            });
            for locator in &claim.record.source_locators {
                if lost.contains(&self.source_locator(&a, *locator).await?.source.source_id) {
                    governed = true;
                }
            }
            for edge in relations.iter().take(64) {
                if !review.relations.iter().any(|r| r.id == edge.id) {
                    review.relations.push(edge.clone());
                }
                let edge = &edge.relation;
                if edge.to.memory_id == r.memory_id
                    && matches!(
                        edge.kind,
                        RelationKind::DerivedFrom | RelationKind::DependsOn
                    )
                {
                    queue.push_back(edge.from.clone());
                }
                if edge.from.memory_id == r.memory_id
                    && matches!(edge.kind, RelationKind::Supports | RelationKind::Challenges)
                {
                    queue.push_back(edge.to.clone());
                }
                if edge.to.memory_id == r.memory_id
                    && edge.kind == RelationKind::Supports
                    && edge.from.memory_id != before.reference.memory_id
                {
                    match self.current_memory(&a, edge.from.memory_id).await {
                        Ok(m) if m.record.availability == Availability::Routine => {
                            remaining.push(m)
                        }
                        Ok(_) => {}
                        Err(Error::Unavailable) => {}
                        Err(e) => return Err(e),
                    }
                }
                if edge.to.memory_id == r.memory_id && edge.kind == RelationKind::Challenges {
                    challenges.push(self.memory(&a, &edge.from).await?);
                }
            }
            for edge in relations
                .into_iter()
                .filter(|e| e.relation.kind == RelationKind::ConflictsWith)
            {
                if !review.conflicts.iter().any(|e| e.relation.id == edge.id) {
                    let claims = vec![
                        self.memory(&a, &edge.relation.from).await?,
                        self.memory(&a, &edge.relation.to).await?,
                    ];
                    review.conflicts.push(ConflictReview {
                        relation: edge,
                        claims,
                    });
                }
            }
            let mut source_groups: BTreeMap<Uuid, Vec<MemoryRef>> = BTreeMap::new();
            let mut supported = vec![];
            for evidence in remaining {
                let mut roots = BTreeSet::new();
                let mut ancestry = vec![evidence.clone()];
                let mut versions = BTreeSet::new();
                let mut complete = true;
                while let Some(m) = ancestry.pop() {
                    if !versions.insert(m.version_id) {
                        continue;
                    }
                    if versions.len() > 32 {
                        complete = false;
                        break;
                    }
                    if m.record.source_locators.is_empty() && m.record.derived_from.is_empty() {
                        complete = false;
                    }
                    for locator in &m.record.source_locators {
                        let src = self.source_locator(&a, *locator).await?.source;
                        let source = self.source_version(&a, &src).await?;
                        let unavailable = if let Some(id) = source.snapshot_artifact {
                            sqlx::query(
                                "SELECT state FROM artifact_records WHERE tenant_id=$1 AND id=$2",
                            )
                            .bind(a.tenant_id.to_string())
                            .bind(id.to_string())
                            .fetch_optional(&self.pool)
                            .await?
                            .is_none_or(|r| r.get::<String, _>("state") != "ready")
                        } else {
                            false
                        };
                        if lost.contains(&src.source_id) || unavailable {
                            complete = false;
                        } else {
                            roots.insert(src.source_id);
                        }
                    }
                    for parent in &m.record.derived_from {
                        ancestry.push(self.memory(&a, parent).await?);
                    }
                }
                if complete && !roots.is_empty() {
                    for root in roots {
                        source_groups
                            .entry(root)
                            .or_default()
                            .push(evidence.reference.clone());
                    }
                    supported.push(evidence);
                }
            }
            review.coverage.examined.push(claim.reference.label.clone());
            review.affected.push(SupportReview {
                claim,
                remaining: supported,
                source_groups,
                governed_derivative: governed,
                challenges,
            });
        }
        let query = ActivationQuery {
            scope: a.scope.clone(),
            question: before.reference.label.clone(),
            task_context: review.request.reason.clone(),
            entities: before.record.scope.entity_ids.clone(),
            exact: vec![],
            families: vec![],
            valid_at: None,
            recorded_as_of: None,
            fresh_after: None,
            vector: None,
            existing: vec![],
            scan_limit: 64,
            candidate_limit: 16,
            traversal_limit: 16,
            context_bytes: 16000,
        };
        let search = self.activation(&a, job.id, query, None).await?;
        let mut indirect = review.request.candidates.clone();
        indirect.extend(search.candidates.iter().map(|c| c.memory.reference.clone()));
        for r in indirect {
            if seen.insert(r.memory_id) {
                if review.indirect.len() >= 16 {
                    review
                        .coverage
                        .unexamined
                        .push("Indirect dependency bound reached".into());
                    break;
                }
                review
                    .indirect
                    .push(self.current_consolidation_memory(&a, &r).await?);
            }
        }
        let text = serde_json::to_string(&review)?;
        if text.len() > 60000 {
            return Err(Error::LimitExceeded);
        }
        sqlx::query(
            "INSERT INTO maintenance_reviews(tenant_id,id,job_id,data) VALUES($1,$2,$3,$4)",
        )
        .bind(a.tenant_id.to_string())
        .bind(id.to_string())
        .bind(job.id.to_string())
        .bind(text)
        .execute(&self.pool)
        .await?;
        Ok(review)
    }
    pub async fn load_maintenance_review(
        &self,
        a: &Authority,
        permit: &Fence,
        id: Uuid,
    ) -> Result<MaintenanceReview> {
        self.assigned_job(a, permit).await?;
        let r = sqlx::query(
            "SELECT data FROM maintenance_reviews WHERE tenant_id=$1 AND id=$2 AND job_id=$3",
        )
        .bind(a.tenant_id.to_string())
        .bind(id.to_string())
        .bind(permit.job_id.to_string())
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        Ok(serde_json::from_str(r.get("data"))?)
    }
    pub async fn commit_maintenance(
        &self,
        a: &Authority,
        permit: &Fence,
        r: &MaintenanceReview,
        d: MaintenanceDisposition,
    ) -> Result<MaintenanceResult> {
        let (mut tx, at) = self.begin_write().await?;
        let job = fence(&mut tx, a, permit, at.recorded_at).await?;
        if job.cancel_requested
            || job.id != r.job_id
            || job.spec.brief.process != Process::Maintenance
        {
            return Err(Error::Forbidden);
        }
        crate::policies::current_policy(&mut tx, a, &job.spec.brief.policy).await?;
        let row =
            sqlx::query("SELECT result FROM maintenance_reviews WHERE tenant_id=$1 AND id=$2")
                .bind(a.tenant_id.to_string())
                .bind(r.id.to_string())
                .fetch_one(&mut *tx)
                .await?;
        if let Some(s) = row.get::<Option<String>, _>("result") {
            return Ok(serde_json::from_str(&s)?);
        }
        let reviewed: Vec<_> = r
            .affected
            .iter()
            .flat_map(|s| std::iter::once(&s.claim).chain(s.remaining.iter()))
            .chain(r.indirect.iter())
            .map(|m| m.reference.clone())
            .collect();
        for edge in &r.relations {
            let revision: i64 =
                sqlx::query("SELECT revision FROM relation_records WHERE tenant_id=$1 AND id=$2")
                    .bind(a.tenant_id.to_string())
                    .bind(edge.id.to_string())
                    .fetch_one(&mut *tx)
                    .await?
                    .get("revision");
            if revision != i64::from(edge.revision) {
                return Err(Error::Conflict);
            }
        }
        for reference in reviewed {
            let m = memory_version(&mut tx, a, &reference).await?;
            if m.recorded_until.is_some() || m.record.availability != Availability::Routine {
                return Err(Error::Conflict);
            }
        }
        // Support may be revoked while semantic review is running. Check its entire
        // retained lineage inside the same write transaction as the resulting revision.
        let mut support: Vec<_> = r
            .affected
            .iter()
            .flat_map(|s| s.remaining.iter())
            .cloned()
            .collect();
        let mut checked = BTreeSet::new();
        while let Some(m) = support.pop() {
            if !checked.insert(m.version_id) {
                continue;
            }
            if checked.len() > 512 {
                return Err(Error::LimitExceeded);
            }
            for id in &m.record.source_locators {
                let locator = crate::sources::locator(&mut tx, a, *id).await?;
                let source = crate::sources::source_version(&mut tx, a, &locator.source).await?;
                if let Some(id) = source.snapshot_artifact {
                    crate::artifacts::ready_artifact(&mut tx, a, id).await?;
                }
            }
            for parent in &m.record.derived_from {
                let m = memory_version(&mut tx, a, parent).await?;
                if m.recorded_until.is_some() || m.record.availability != Availability::Routine {
                    return Err(Error::Conflict);
                }
                support.push(m);
            }
        }
        let mut result = MaintenanceResult {
            review_id: r.id,
            kind: d.kind,
            changed: vec![],
            preserved: vec![],
            notices: vec![],
            revalidation: vec![],
            unresolved: r.coverage.unexamined.clone(),
            coverage: r.coverage.clone(),
        };
        let mut changes = vec![];
        let mut planned: Vec<(MemoryRef, ValidTime)> = vec![];
        if d.kind == ChangeKind::Unresolved {
            result
                .unresolved
                .push("Change classification remains unresolved".into());
        } else if let Some(after) = &r.request.after {
            let mut after = after.clone();
            after.decision.reason = r.request.reason.clone();
            if d.kind == ChangeKind::Wording
                && (after.valid_time != r.before.record.valid_time
                    || after.availability != r.before.record.availability
                    || after.derived_from != r.before.record.derived_from
                    || after.source_locators != r.before.record.source_locators
                    || serde_json::to_value(&after.qualification)?
                        != serde_json::to_value(&r.before.record.qualification)?
                    || serde_json::to_value(after.origin)?
                        != serde_json::to_value(r.before.record.origin)?
                    || serde_json::to_value(after.evidential_status)?
                        != serde_json::to_value(r.before.record.evidential_status)?)
            {
                return Err(Error::Invalid(
                    "Wording-only changes must preserve time, availability and evidence".into(),
                ));
            }
            if d.kind == ChangeKind::WorldChange {
                let boundary = match &after.valid_time {
                    ValidTime::Interval { from: Some(at), .. } => *at,
                    _ => {
                        return Err(Error::Invalid(
                            "World changes need a known new valid interval".into(),
                        ));
                    }
                };
                let mut prior = r.before.record.clone();
                prior.valid_time = match prior.valid_time {
                    ValidTime::Interval { from, to }
                        if from.is_none_or(|f| f < boundary)
                            && to.is_none_or(|t| boundary <= t) =>
                    {
                        ValidTime::Interval {
                            from,
                            to: Some(boundary),
                        }
                    }
                    _ => {
                        return Err(Error::Invalid(
                            "World change must close the prior valid interval".into(),
                        ));
                    }
                };
                changes.push(MemoryChange::Revise {
                    expected: r.before.reference.clone(),
                    record: prior,
                });
                planned.push((r.before.reference.clone(), after.valid_time.clone()));
                changes.push(MemoryChange::Create(after));
            } else {
                planned.push((r.before.reference.clone(), after.valid_time.clone()));
                changes.push(MemoryChange::Revise {
                    expected: r.before.reference.clone(),
                    record: after,
                });
            }
        }
        if !matches!(d.kind, ChangeKind::Wording | ChangeKind::Unresolved) {
            for support in &r.affected {
                if support.claim.reference == r.before.reference && r.request.after.is_some() {
                    continue;
                }
                let answer = d
                    .support
                    .get(&support.claim.reference.memory_id)
                    .map(String::as_str);
                let source_loss = !r.request.removed_sources.is_empty();
                let keep = answer == Some("adequate")
                    && !support.remaining.is_empty()
                    && !(source_loss && support.governed_derivative);
                if keep {
                    let mut record = support.claim.record.clone();
                    record.decision.reason =
                        "Independent remaining support was reassessed as adequate".into();
                    record.decision.constraints.push(format!(
                        "Remaining source groups: {}",
                        support.source_groups.len()
                    ));
                    planned.push((support.claim.reference.clone(), record.valid_time.clone()));
                    changes.push(MemoryChange::Revise {
                        expected: support.claim.reference.clone(),
                        record,
                    });
                    result.preserved.push(support.claim.reference.clone());
                    continue;
                }
                let mut record = support.claim.record.clone();
                record.availability = Availability::Retired;
                if matches!(record.content, MemoryContent::Procedure { .. }) {
                    record.qualification = Qualification::Withdrawn {
                        reason: r.request.reason.clone(),
                    };
                }
                record.decision.reason = format!(
                    "Routine guidance retired pending revalidation: {}",
                    r.request.reason
                );
                record.decision.constraints.push(format!(
                    "Remaining support: {}",
                    answer.unwrap_or("unresolved")
                ));
                planned.push((support.claim.reference.clone(), record.valid_time.clone()));
                changes.push(MemoryChange::Revise {
                    expected: support.claim.reference.clone(),
                    record,
                });
                result
                    .unresolved
                    .push(format!("Revalidate {}", support.claim.reference.label));
            }
        } else {
            result.preserved.extend(
                r.affected
                    .iter()
                    .filter(|s| {
                        s.claim.reference != r.before.reference || d.kind == ChangeKind::Unresolved
                    })
                    .map(|s| s.claim.reference.clone()),
            );
        }
        for change in &changes {
            let record = match change {
                MemoryChange::Create(r) | MemoryChange::Revise { record: r, .. } => r,
            };
            crate::intentions::check_plan(&mut tx, a, &job, record).await?;
        }
        if !changes.is_empty() {
            result.changed = apply_changes(&mut tx, a, changes, at).await?;
        }
        let retired: Vec<_> = result
            .changed
            .iter()
            .filter(|m| m.record.availability == Availability::Retired)
            .cloned()
            .collect();
        for retired in retired {
            let mut obligation = retired.record.clone();
            obligation.label = format!("Revalidate {}", retired.reference.label);
            obligation.content = MemoryContent::Intention {
                purpose: obligation.label.clone(),
                owner_id: a.actor_id,
                trigger: "Evidence or an execution plan becomes available".into(),
                readiness: vec!["Obtain accessible evidence and an approved execution plan".into()],
                completion: vec![
                    "Reassess the retired claim against the available evidence".into(),
                ],
                expires_at: None,
                notification_policy: "quiet".into(),
                recurrence: None,
                plan: None,
            };
            obligation.availability = Availability::Routine;
            obligation.qualification = Qualification::Candidate;
            obligation.source_locators.clear();
            obligation.derived_from = vec![retired.reference.clone()];
            obligation.decision.reason = r.request.reason.clone();
            let retained = apply_changes(&mut tx, a, vec![MemoryChange::Create(obligation)], at)
                .await?
                .remove(0);
            result.revalidation.push(retained.reference);
        }
        if d.kind == ChangeKind::WorldChange {
            crate::relations::insert_relation(
                &mut tx,
                a,
                RelationDraft {
                    from: result.changed[1].reference.clone(),
                    to: result.changed[0].reference.clone(),
                    kind: RelationKind::Supersedes,
                    scope: r.before.record.scope.clone(),
                    basis: r.request.reason.clone(),
                    evidential_status: EvidentialStatus::Inference,
                    acceptance: Acceptance::Accepted,
                    valid_time: result.changed[1].record.valid_time.clone(),
                },
                at,
            )
            .await?;
        }
        if d.kind == ChangeKind::Unresolved && d.comparison.as_deref() == Some("incompatible") {
            if let Some(after) = &r.request.after {
                if !overlaps(&r.before.record.valid_time, &after.valid_time) {
                    return Err(Error::Invalid(
                        "Different valid intervals do not establish a conflict".into(),
                    ));
                }
                let mut disputed = after.clone();
                disputed.qualification = Qualification::Candidate;
                let new = apply_changes(&mut tx, a, vec![MemoryChange::Create(disputed)], at)
                    .await?
                    .remove(0);
                crate::relations::insert_relation(
                    &mut tx,
                    a,
                    RelationDraft {
                        from: r.before.reference.clone(),
                        to: new.reference.clone(),
                        kind: RelationKind::ConflictsWith,
                        scope: r.before.record.scope.clone(),
                        basis: r.request.reason.clone(),
                        evidential_status: EvidentialStatus::Inference,
                        acceptance: Acceptance::Accepted,
                        valid_time: r.before.record.valid_time.clone(),
                    },
                    at,
                )
                .await?;
                result.changed.push(new);
                planned.push((
                    r.before.reference.clone(),
                    r.before.record.valid_time.clone(),
                ));
            }
        }
        let primary = result
            .changed
            .iter()
            .find(|m| m.reference.memory_id == r.before.reference.memory_id)
            .map(|m| m.reference.clone())
            .unwrap_or(r.before.reference.clone());
        for candidate in &r.indirect {
            if d.kind == ChangeKind::Wording {
                result.preserved.push(candidate.reference.clone());
                continue;
            }
            match d
                .indirect
                .get(&candidate.reference.memory_id)
                .map(String::as_str)
            {
                Some("affected") => {
                    crate::relations::insert_relation(
                        &mut tx,
                        a,
                        RelationDraft {
                            from: candidate.reference.clone(),
                            to: primary.clone(),
                            kind: RelationKind::DependsOn,
                            scope: candidate.record.scope.clone(),
                            basis: r.request.reason.clone(),
                            evidential_status: EvidentialStatus::Inference,
                            acceptance: Acceptance::Candidate,
                            valid_time: candidate.record.valid_time.clone(),
                        },
                        at,
                    )
                    .await?;
                    result.unresolved.push(format!(
                        "Investigate indirect dependency: {}",
                        candidate.reference.label
                    ));
                }
                Some("unaffected") => result.preserved.push(candidate.reference.clone()),
                _ => result.unresolved.push(format!(
                    "Indirect dependency unresolved: {}",
                    candidate.reference.label
                )),
            }
        }
        for review in &r.conflicts {
            let conflict = &review.relation;
            match d.conflicts.get(&conflict.id).map(String::as_str) {
                Some("compatible" | "different_scope_or_time") => {
                    let mut relation = conflict.relation.clone();
                    relation.acceptance = Acceptance::Withdrawn;
                    relation.basis = r.request.reason.clone();
                    crate::relations::revise_relation_in(
                        &mut tx,
                        a,
                        conflict.id,
                        conflict.revision,
                        relation,
                        at,
                    )
                    .await?;
                }
                _ => result
                    .unresolved
                    .push(format!("Conflict remains open: {}", conflict.id)),
            }
        }
        for (previous, valid_time) in planned {
            let current = if d.kind == ChangeKind::WorldChange && previous == r.before.reference {
                result
                    .changed
                    .iter()
                    .find(|m| m.reference.memory_id != previous.memory_id)
            } else {
                result
                    .changed
                    .iter()
                    .find(|m| m.reference.memory_id == previous.memory_id)
            }
            .or_else(|| (d.kind == ChangeKind::Unresolved).then_some(&r.before))
            .ok_or(Error::Conflict)?;
            crate::coordinator::events::append_event(
                &mut tx,
                a,
                current.reference.memory_id,
                "memory_maintained",
                at,
                at.recorded_at + Duration::days(30),
            )
            .await?;
            let cursor = crate::database::number(
                sqlx::query("SELECT sequence FROM commit_clock WHERE id=1")
                    .fetch_one(&mut *tx)
                    .await?
                    .get("sequence"),
            )?;
            let notice = MemoryChangeNotice {
                id: Uuid::now_v7(),
                cursor,
                kind: d.kind,
                previous,
                current: current.reference.clone(),
                scope: current.record.scope.clone(),
                valid_time,
                reason: r.request.reason.clone(),
            };
            sqlx::query("INSERT INTO memory_changes(tenant_id,id,cursor,resource_id,data) VALUES($1,$2,$3,$4,$5)").bind(a.tenant_id.to_string()).bind(notice.id.to_string()).bind(i64::from(cursor)).bind(current.reference.memory_id.to_string()).bind(serde_json::to_string(&notice)?).execute(&mut *tx).await?;
            result.notices.push(notice);
        }
        sqlx::query("UPDATE maintenance_reviews SET result=$1 WHERE tenant_id=$2 AND id=$3")
            .bind(serde_json::to_string(&result)?)
            .bind(a.tenant_id.to_string())
            .bind(r.id.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(result)
    }
    pub async fn memory_changes(
        &self,
        a: &Authority,
        after: u32,
        limit: u16,
    ) -> Result<ChangePage> {
        let page = self.events(a, after, limit).await?;
        let mut changes = vec![];
        for event in page.events {
            if event.kind != "memory_maintained" {
                continue;
            }
            if let Some(row) =
                sqlx::query("SELECT data FROM memory_changes WHERE tenant_id=$1 AND cursor=$2")
                    .bind(a.tenant_id.to_string())
                    .bind(i64::from(event.cursor))
                    .fetch_optional(&self.pool)
                    .await?
            {
                changes.push(serde_json::from_str(row.get("data"))?);
            }
        }
        Ok(ChangePage {
            changes,
            cursor: page.cursor,
            snapshot_required: page.snapshot_required,
        })
    }
}
impl Store {
    pub async fn current_memory(&self, a: &Authority, id: Uuid) -> Result<MemoryVersion> {
        let mut c = self.pool.acquire().await?;
        crate::access::read_scope(&mut c, a, id).await?;
        let row = sqlx::query("SELECT revision FROM memory_records WHERE tenant_id=$1 AND id=$2")
            .bind(a.tenant_id.to_string())
            .bind(id.to_string())
            .fetch_optional(&mut *c)
            .await?
            .ok_or(Error::NotFound)?;
        let revision = crate::database::number(row.get("revision"))?
            .try_into()
            .map_err(|_| Error::Conflict)?;
        memory_version(
            &mut c,
            a,
            &MemoryRef {
                memory_id: id,
                revision,
                label: String::new(),
            },
        )
        .await
    }
}

fn overlaps(a: &ValidTime, b: &ValidTime) -> bool {
    match (a, b) {
        (ValidTime::Interval { from: af, to: at }, ValidTime::Interval { from: bf, to: bt }) => {
            af.zip(*bt).is_none_or(|(f, t)| f < t) && bf.zip(*at).is_none_or(|(f, t)| f < t)
        }
        _ => true,
    }
}
