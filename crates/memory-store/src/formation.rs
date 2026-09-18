use crate::{Error, Result, Store, coordinator::fence};
use memory_domain::{contracts::*, coordination::Fence, formation::*, records::*};
use sqlx::{AnyConnection, Row};
use uuid::Uuid;

impl Store {
    pub async fn formation_duplicate(
        &self,
        auth: &Authority,
        window: &FormationWindow,
        event: &memory_domain::sources::ToolEvent,
    ) -> Result<Option<(Vec<MemoryRef>, Option<DeferredContribution>)>> {
        let mut c = self.pool.acquire().await?;
        if let Some((prior, refs, deferred)) =
            event_records(&mut c, auth, window, &event.event_id).await?
        {
            if prior != serde_json::to_value(event)? {
                return Err(Error::Conflict);
            }
            for r in &refs {
                crate::memories::memory_version(&mut c, auth, r).await?;
            }
            return Ok(Some((refs, deferred)));
        }
        Ok(None)
    }

    pub async fn formation_cursor(
        &self,
        auth: &Authority,
        source: &SourceRef,
        operation: &str,
        policy: &ConfigRef,
    ) -> Result<u32> {
        crate::sources::source_version(&mut *self.pool.acquire().await?, auth, source).await?;
        cursor(
            &mut *self.pool.acquire().await?,
            auth,
            source,
            operation,
            policy,
        )
        .await
    }
    pub async fn formation_window(
        &self,
        auth: &Authority,
        permit: &Fence,
        id: Uuid,
    ) -> Result<Option<FormationWindow>> {
        self.assigned_job(auth, permit).await?;
        let row = sqlx::query(
            "SELECT data FROM formation_windows WHERE tenant_id=$1 AND id=$2 AND job_id=$3",
        )
        .bind(auth.tenant_id.to_string())
        .bind(id.to_string())
        .bind(permit.job_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(|r| serde_json::from_str(r.get("data")).map_err(Error::from))
            .transpose()
    }
    /// Only the trusted source reader admits a window; workers receive its immutable artifact.
    pub async fn save_formation_window(
        &self,
        auth: &Authority,
        permit: &Fence,
        window: &FormationWindow,
    ) -> Result<()> {
        let (mut tx, at) = self.begin_write().await?;
        let job = fence(&mut tx, auth, permit, at.recorded_at).await?;
        if job.cancel_requested
            || job.deadline <= at.recorded_at
            || window.job_id != job.id
            || window.policy != job.spec.brief.policy
        {
            return Err(Error::Conflict);
        }
        sqlx::query("INSERT INTO formation_windows(tenant_id,id,job_id,data) VALUES($1,$2,$3,$4)")
            .bind(auth.tenant_id.to_string())
            .bind(window.id.to_string())
            .bind(job.id.to_string())
            .bind(serde_json::to_string(window)?)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn formation_result(
        &self,
        auth: &Authority,
        permit: &Fence,
        request: &FormationCommit,
    ) -> Result<Option<FormationResult>> {
        self.assigned_job(auth, permit).await?;
        existing(&mut *self.pool.acquire().await?, auth, permit, request).await
    }
    pub async fn judgement_decision(
        &self,
        auth: &Authority,
        permit: &Fence,
        id: Uuid,
    ) -> Result<memory_domain::judgement::JudgementDecision> {
        self.assigned_job(auth, permit).await?;
        let row = sqlx::query(
            "SELECT data FROM judgement_decisions WHERE tenant_id=$1 AND id=$2 AND job_id=$3",
        )
        .bind(auth.tenant_id.to_string())
        .bind(id.to_string())
        .bind(permit.job_id.to_string())
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        Ok(serde_json::from_str(row.get("data"))?)
    }
    /// Record writes, duplicate receipt and cursor advancement share one transaction.
    /// `accepted` comes from the host's checked assessment, never the HTTP caller.
    pub async fn commit_formation(
        &self,
        auth: &Authority,
        permit: &Fence,
        window: &FormationWindow,
        request: &FormationCommit,
        accepted: &std::collections::BTreeMap<String, PolicyDecision>,
        mut result: FormationResult,
    ) -> Result<FormationResult> {
        let (mut tx, at) = self.begin_write().await?;
        let job = fence(&mut tx, auth, permit, at.recorded_at).await?;
        if let Some(prior) = existing(&mut tx, auth, permit, request).await? {
            return Ok(prior);
        }
        if job.cancel_requested
            || job.deadline <= at.recorded_at
            || window.job_id != job.id
            || window.policy != job.spec.brief.policy
            || window.cutoff > job.spec.brief.evidence_cutoff
        {
            return Err(Error::Conflict);
        }
        let narrow = Authority {
            scope: window.scope.clone(),
            ..auth.clone()
        };
        let source = crate::sources::source_version(&mut tx, &narrow, &window.source).await?;
        crate::artifacts::ready_artifact(
            &mut tx,
            &narrow,
            source.snapshot_artifact.ok_or(Error::Unavailable)?,
        )
        .await?;
        if cursor(
            &mut tx,
            &narrow,
            &window.source,
            &window.operation,
            &window.policy,
        )
        .await?
            != window.after
        {
            return Err(Error::Conflict);
        }
        for entry in &window.entries {
            let prior = event_records(&mut tx, &narrow, window, &entry.event.event_id).await?;
            if let Some((event, refs, _)) = prior {
                if event != serde_json::to_value(&entry.event)? {
                    return Err(Error::Conflict);
                }
                for r in &refs {
                    crate::memories::memory_version(&mut tx, &narrow, r).await?;
                }
                result
                    .duplicate_events
                    .insert(entry.event.event_id.clone(), refs);
                continue;
            }
            let mut refs = Vec::new();
            if let Some(decision) = accepted.get(&entry.event.event_id) {
                // Recheck all native source snapshots in the same transaction as retention.
                for native in &entry.native_locators {
                    let locator = crate::sources::locator(&mut tx, &narrow, native.id).await?;
                    let version =
                        crate::sources::source_version(&mut tx, &narrow, &locator.source).await?;
                    if let Some(id) = version.snapshot_artifact {
                        crate::artifacts::ready_artifact(&mut tx, &narrow, id).await?;
                    }
                }
                let mut corrected = Vec::new();
                for id in &entry.input.corrects {
                    let (_, targets, _) = event_records(&mut tx, &narrow, window, id)
                        .await?
                        .ok_or_else(|| {
                            Error::Invalid(format!("Correction target {id} has not been retained"))
                        })?;
                    if targets.is_empty() {
                        return Err(Error::Invalid("Correction target was deferred".into()));
                    }
                    corrected.extend(targets);
                }
                let mut draft = entry.record(window, decision.clone());
                for id in &entry.input.based_on {
                    let (_, references, _) = event_records(&mut tx, &narrow, window, id)
                        .await?
                        .ok_or_else(|| {
                            Error::Invalid(format!("Derivation target {id} has not been retained"))
                        })?;
                    if references.is_empty() {
                        return Err(Error::Invalid("Derivation target was deferred".into()));
                    }
                    for reference in references {
                        let input =
                            crate::memories::memory_version(&mut tx, &narrow, &reference).await?;
                        if input.record.evidential_status == EvidentialStatus::Simulation
                            && draft.evidential_status != EvidentialStatus::Simulation
                        {
                            return Err(Error::Invalid(
                                "Derived records must preserve simulation status".into(),
                            ));
                        }
                        if !draft.derived_from.contains(&reference) {
                            draft.derived_from.push(reference);
                        }
                    }
                }
                crate::intentions::check_plan(&mut tx, auth, &job, &draft).await?;
                let version = crate::memories::apply_changes(
                    &mut tx,
                    &narrow,
                    vec![MemoryChange::Create(draft)],
                    at,
                )
                .await?
                .remove(0);
                for target in corrected {
                    crate::relations::insert_relation(&mut tx, &narrow, RelationDraft {
                        from: version.reference.clone(), to: target, kind: RelationKind::Challenges,
                        scope: window.scope.clone(), basis: "Explicit correction in captured event; original status and version retained".into(),
                        evidential_status: entry.event.evidential_status, acceptance: Acceptance::Candidate, valid_time: ValidTime::Unknown,
                    }, at).await?;
                }
                refs.push(version.reference.clone());
                result.records.push(version);
            }
            sqlx::query("INSERT INTO formation_events(tenant_id,source_id,operation,policy_id,policy_revision,event_id,event,records,deferred) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
                .bind(auth.tenant_id.to_string()).bind(window.source.source_id.to_string()).bind(&window.operation).bind(window.policy.id.to_string()).bind(i64::from(window.policy.revision.get())).bind(&entry.event.event_id).bind(serde_json::to_string(&entry.event)?).bind(serde_json::to_string(&refs)?).bind(serde_json::to_string(&result.deferred.iter().find(|d| d.event_id == entry.event.event_id))?).execute(&mut *tx).await?;
        }
        sqlx::query("INSERT INTO formation_cursors(tenant_id,source_id,source_revision,operation,policy_id,policy_revision,cursor) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(tenant_id,source_id,source_revision,operation,policy_id,policy_revision) DO UPDATE SET cursor=excluded.cursor")
            .bind(auth.tenant_id.to_string()).bind(window.source.source_id.to_string()).bind(&window.source.revision).bind(&window.operation).bind(window.policy.id.to_string()).bind(i64::from(window.policy.revision.get())).bind(i64::from(window.through)).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO formation_results(tenant_id,window_id,job_id,request,result) VALUES($1,$2,$3,$4,$5)")
            .bind(auth.tenant_id.to_string()).bind(window.id.to_string()).bind(job.id.to_string()).bind(serde_json::to_string(request)?).bind(serde_json::to_string(&result)?).execute(&mut *tx).await?;
        if !result.records.is_empty() {
            crate::coordinator::events::append_event(
                &mut tx,
                auth,
                job.id,
                "memory_changed",
                at,
                job.spec.retain_until,
            )
            .await?;
        }
        tx.commit().await?;
        Ok(result)
    }
}
async fn cursor(
    c: &mut AnyConnection,
    a: &Authority,
    s: &SourceRef,
    operation: &str,
    p: &ConfigRef,
) -> Result<u32> {
    let row = sqlx::query("SELECT cursor FROM formation_cursors WHERE tenant_id=$1 AND source_id=$2 AND source_revision=$3 AND operation=$4 AND policy_id=$5 AND policy_revision=$6")
        .bind(a.tenant_id.to_string()).bind(s.source_id.to_string()).bind(&s.revision).bind(operation).bind(p.id.to_string()).bind(i64::from(p.revision.get())).fetch_optional(c).await?;
    row.map(|r| crate::database::number(r.get("cursor")))
        .transpose()
        .map(|v| v.unwrap_or(0))
}
async fn event_records(
    c: &mut AnyConnection,
    a: &Authority,
    w: &FormationWindow,
    id: &str,
) -> Result<
    Option<(
        serde_json::Value,
        Vec<MemoryRef>,
        Option<DeferredContribution>,
    )>,
> {
    let row = sqlx::query("SELECT event,records,deferred FROM formation_events WHERE tenant_id=$1 AND source_id=$2 AND operation=$3 AND policy_id=$4 AND policy_revision=$5 AND event_id=$6")
        .bind(a.tenant_id.to_string()).bind(w.source.source_id.to_string()).bind(&w.operation).bind(w.policy.id.to_string()).bind(i64::from(w.policy.revision.get())).bind(id).fetch_optional(c).await?;
    row.map(|r| {
        Ok((
            serde_json::from_str(r.get("event"))?,
            serde_json::from_str(r.get("records"))?,
            serde_json::from_str(r.get("deferred"))?,
        ))
    })
    .transpose()
}
async fn existing(
    c: &mut AnyConnection,
    a: &Authority,
    f: &Fence,
    request: &FormationCommit,
) -> Result<Option<FormationResult>> {
    let row = sqlx::query("SELECT request,result FROM formation_results WHERE tenant_id=$1 AND window_id=$2 AND job_id=$3")
        .bind(a.tenant_id.to_string()).bind(request.window_id.to_string()).bind(f.job_id.to_string()).fetch_optional(c).await?;
    row.map(|r| {
        if serde_json::from_str::<serde_json::Value>(r.get("request"))?
            != serde_json::to_value(request)?
        {
            return Err(Error::Conflict);
        }
        Ok(serde_json::from_str(r.get("result"))?)
    })
    .transpose()
}
