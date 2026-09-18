use super::*;
use crate::{
    access::{require_write_scope, write_scope},
    coordinator::{events::append_event, receipts},
    database::changed,
};
use chrono::Duration;

impl Store {
    pub(super) async fn user_mutation(
        &self,
        auth: &Authority,
        request_id: Uuid,
        mutation: UserMutation,
    ) -> Result<UserResponse> {
        let (mut tx, pos) = self.begin_write().await?;
        // Receipts are per actor: an identical request from another user is not their contribution.
        let request = serde_json::json!({"actor_id":auth.actor_id,"mutation":mutation});
        if let Some(prior) = receipts::existing(&mut tx, auth, request_id, "user", &request).await?
        {
            return Ok(prior);
        }
        use UserMutation as M;
        use UserResponse as O;
        let mut resource = request_id;
        let mut event = None;
        let result = match mutation {
            M::Contribute { mut record } => {
                human_record(auth, &mut record)?;
                let memory = crate::memories::apply_changes(
                    &mut tx,
                    auth,
                    vec![MemoryChange::Create(*record)],
                    pos,
                )
                .await?
                .remove(0);
                resource = memory.reference.memory_id;
                event = Some("user_memory_changed");
                O::Memory {
                    memory: Box::new(memory),
                    relations: vec![],
                }
            }
            M::Correct {
                expected,
                mut record,
            } => {
                human_record(auth, &mut record)?;
                let memory = crate::memories::apply_changes(
                    &mut tx,
                    auth,
                    vec![MemoryChange::Revise {
                        expected: expected.clone(),
                        record: *record,
                    }],
                    pos,
                )
                .await?
                .remove(0);
                resource = memory.reference.memory_id;
                append_event(
                    &mut tx,
                    auth,
                    resource,
                    "memory_maintained",
                    pos,
                    pos.recorded_at + Duration::days(30),
                )
                .await?;
                let cursor = crate::database::number(
                    sqlx::query("SELECT sequence FROM commit_clock WHERE id=1")
                        .fetch_one(&mut *tx)
                        .await?
                        .try_get("sequence")?,
                )?;
                let notice = memory_domain::maintenance::MemoryChangeNotice {
                    id: Uuid::now_v7(),
                    cursor,
                    kind: memory_domain::maintenance::ChangeKind::Correction,
                    previous: expected,
                    current: memory.reference.clone(),
                    scope: memory.record.scope.clone(),
                    valid_time: memory.record.valid_time.clone(),
                    reason: memory.record.decision.reason.clone(),
                };
                sqlx::query("INSERT INTO memory_changes(tenant_id,id,cursor,resource_id,data) VALUES($1,$2,$3,$4,$5)").bind(auth.tenant_id.to_string()).bind(notice.id.to_string()).bind(i64::from(cursor)).bind(resource.to_string()).bind(serde_json::to_string(&notice)?).execute(&mut *tx).await?;
                O::Memory {
                    memory: Box::new(memory),
                    relations: vec![],
                }
            }
            M::CreatePolicy {
                label,
                scope,
                policy,
                effective,
            } => {
                let policy = crate::policies::create_policy_in(
                    &mut tx, auth, &label, scope, *policy, effective, pos,
                )
                .await?;
                resource = policy.reference.id;
                O::Policy {
                    policy: Box::new(policy),
                }
            }
            M::RevisePolicy {
                expected,
                policy,
                effective,
            } => {
                let policy = crate::policies::revise_policy_in(
                    &mut tx, auth, &expected, *policy, effective, pos,
                )
                .await?;
                resource = policy.reference.id;
                O::Policy {
                    policy: Box::new(policy),
                }
            }
            M::OpenExploration {
                scope,
                purpose,
                expires_at,
            } => {
                text(&purpose)?;
                if expires_at <= pos.recorded_at
                    || expires_at > pos.recorded_at + Duration::days(30)
                {
                    return Err(Error::Invalid(
                        "Exploration expiry must be within 30 days".into(),
                    ));
                }
                resource = Uuid::now_v7();
                write_scope(&mut tx, auth, resource, &scope).await?;
                let exploration = Exploration {
                    id: resource,
                    revision: 1,
                    scope,
                    purpose,
                    expires_at,
                    records: vec![],
                    promoted: vec![],
                };
                sqlx::query(
                    "INSERT INTO explorations (tenant_id,id,revision,data) VALUES ($1,$2,1,$3)",
                )
                .bind(auth.tenant_id.to_string())
                .bind(resource.to_string())
                .bind(serde_json::to_string(&exploration)?)
                .execute(&mut *tx)
                .await?;
                O::Exploration {
                    exploration: Box::new(exploration),
                }
            }
            M::AddExploration {
                id,
                expected_revision,
                mut record,
            } => {
                require_write_scope(&mut tx, auth, id).await?;
                let mut exploration = exploration(&mut tx, auth, id).await?;
                if exploration.revision != expected_revision {
                    return Err(Error::Conflict);
                }
                if exploration.records.len() >= 16 {
                    return Err(Error::LimitExceeded);
                }
                human_record(auth, &mut record)?;
                if record.scope != exploration.scope {
                    return Err(Error::Forbidden);
                }
                record.evidential_status = EvidentialStatus::Assumption;
                crate::memories::validate_record(&mut tx, auth, &record, pos).await?;
                exploration.records.push(*record);
                save_exploration(&mut tx, auth, &mut exploration).await?;
                resource = id;
                O::Exploration {
                    exploration: Box::new(exploration),
                }
            }
            M::PromoteExploration {
                id,
                expected_revision,
                index,
            } => {
                require_write_scope(&mut tx, auth, id).await?;
                let mut exploration = exploration(&mut tx, auth, id).await?;
                if exploration.revision != expected_revision
                    || exploration.promoted.contains(&index)
                {
                    return Err(Error::Conflict);
                }
                let record = exploration
                    .records
                    .get(usize::from(index))
                    .ok_or(Error::NotFound)?
                    .clone();
                let memory = crate::memories::apply_changes(
                    &mut tx,
                    auth,
                    vec![MemoryChange::Create(record)],
                    pos,
                )
                .await?
                .remove(0);
                exploration.promoted.push(index);
                save_exploration(&mut tx, auth, &mut exploration).await?;
                resource = id;
                event = Some("exploration_promoted");
                O::Memory {
                    memory: Box::new(memory),
                    relations: vec![],
                }
            }
            M::RequestDecision {
                job_id,
                fence,
                owner_id,
                question,
                missing,
                deadline,
            } => {
                if let Some(fence) = fence {
                    if fence.job_id != job_id {
                        return Err(Error::Forbidden);
                    }
                    crate::coordinator::fence(&mut tx, auth, &fence, pos.recorded_at).await?;
                }
                require_write_scope(&mut tx, auth, job_id).await?;
                let job = crate::coordinator::job(&mut tx, auth, job_id).await?;
                text(&question)?;
                text(&missing)?;
                if deadline <= pos.recorded_at || deadline > job.deadline {
                    return Err(Error::Invalid(
                        "Decision deadline must be in the future and within the job deadline"
                            .into(),
                    ));
                }
                resource = Uuid::now_v7();
                write_scope(&mut tx, auth, resource, &job.spec.brief.scope).await?;
                let decision = DecisionRequest {
                    id: resource,
                    revision: 1,
                    job_id,
                    scope: job.spec.brief.scope,
                    owner_id,
                    question,
                    missing,
                    deadline,
                    answer: None,
                    reason: None,
                    answered_by: None,
                    fallback: "Keep this job blocked; owner must cancel an expired or declined job"
                        .into(),
                };
                sqlx::query("INSERT INTO decision_requests (tenant_id,id,job_id,revision,answer,data) VALUES ($1,$2,$3,1,NULL,$4)").bind(auth.tenant_id.to_string()).bind(resource.to_string()).bind(job_id.to_string()).bind(serde_json::to_string(&decision)?).execute(&mut *tx).await?;
                event = Some("decision_needed");
                O::Decisions {
                    decisions: vec![decision],
                }
            }
            M::AnswerDecision {
                id,
                expected_revision,
                answer,
                reason,
            } => {
                require_write_scope(&mut tx, auth, id).await?;
                text(&reason)?;
                let row =
                    sqlx::query("SELECT data FROM decision_requests WHERE tenant_id=$1 AND id=$2")
                        .bind(auth.tenant_id.to_string())
                        .bind(id.to_string())
                        .fetch_optional(&mut *tx)
                        .await?
                        .ok_or(Error::NotFound)?;
                let mut decision: DecisionRequest = serde_json::from_str(row.try_get("data")?)?;
                if decision.owner_id != auth.actor_id {
                    return Err(Error::Forbidden);
                }
                if decision.revision != expected_revision
                    || decision.answer.is_some()
                    || decision.deadline <= pos.recorded_at
                {
                    return Err(Error::Conflict);
                }
                decision.revision += 1;
                decision.answer = Some(answer);
                decision.reason = Some(reason);
                decision.answered_by = Some(auth.actor_id);
                changed(sqlx::query("UPDATE decision_requests SET revision=$1,answer=$2,data=$3 WHERE tenant_id=$4 AND id=$5 AND revision=$6").bind(i64::from(decision.revision)).bind(tag(&answer)?).bind(serde_json::to_string(&decision)?).bind(auth.tenant_id.to_string()).bind(id.to_string()).bind(i64::from(expected_revision)).execute(&mut *tx).await?.rows_affected())?;
                resource = id;
                event = Some("decision_answered");
                O::Decisions {
                    decisions: vec![decision],
                }
            }
            M::NotificationPreference { mode } => {
                sqlx::query("INSERT INTO notification_preferences (tenant_id,actor_id,mode) VALUES ($1,$2,$3) ON CONFLICT (tenant_id,actor_id) DO UPDATE SET mode=excluded.mode").bind(auth.tenant_id.to_string()).bind(auth.actor_id.to_string()).bind(tag(&mode)?).execute(&mut *tx).await?;
                O::Done
            }
            M::AcknowledgeNotification { event_id } => {
                let row = sqlx::query(
                    "SELECT resource_id FROM outbox_events WHERE tenant_id=$1 AND id=$2",
                )
                .bind(auth.tenant_id.to_string())
                .bind(event_id.to_string())
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(Error::NotFound)?;
                resource = id(row.try_get("resource_id")?)?;
                read_scope(&mut tx, auth, resource).await?;
                sqlx::query("INSERT INTO notification_deliveries (tenant_id,actor_id,event_id) VALUES ($1,$2,$3) ON CONFLICT DO NOTHING").bind(auth.tenant_id.to_string()).bind(auth.actor_id.to_string()).bind(event_id.to_string()).execute(&mut *tx).await?;
                O::Done
            }
            M::SetControl {
                kind,
                target,
                expected_revision,
                enabled,
            } => {
                if auth.scope != Scope::default() {
                    return Err(Error::Forbidden);
                }
                text(&target)?;
                if expected_revision == 0 {
                    changed(sqlx::query("INSERT INTO runtime_controls (tenant_id,kind,target,revision,enabled) VALUES ($1,$2,$3,1,$4) ON CONFLICT DO NOTHING").bind(auth.tenant_id.to_string()).bind(tag(&kind)?).bind(&target).bind(i64::from(enabled)).execute(&mut *tx).await?.rows_affected())?;
                } else {
                    changed(sqlx::query("UPDATE runtime_controls SET revision=revision+1,enabled=$1 WHERE tenant_id=$2 AND kind=$3 AND target=$4 AND revision=$5").bind(i64::from(enabled)).bind(auth.tenant_id.to_string()).bind(tag(&kind)?).bind(&target).bind(i64::from(expected_revision)).execute(&mut *tx).await?.rows_affected())?;
                }
                O::Controls {
                    controls: vec![RuntimeControl {
                        kind,
                        target,
                        revision: expected_revision
                            .checked_add(1)
                            .ok_or(Error::LimitExceeded)?,
                        enabled,
                    }],
                }
            }
        };
        if resource == request_id {
            write_scope(&mut tx, auth, resource, &auth.scope).await?;
        }
        receipts::save(
            &mut tx, auth, request_id, resource, "user", &request, &result,
        )
        .await?;
        if let Some(kind) = event {
            append_event(
                &mut tx,
                auth,
                resource,
                kind,
                pos,
                pos.recorded_at + Duration::days(30),
            )
            .await?;
        }
        tx.commit().await?;
        Ok(result)
    }
}
fn human_record(auth: &Authority, record: &mut RecordDraft) -> Result<()> {
    if let MemoryContent::Intention { owner_id, .. } = &record.content {
        if *owner_id != auth.actor_id {
            return Err(Error::Forbidden);
        }
    }
    record.origin = Origin::Observed;
    // Preserve explicit uncertainty. A user's contribution alone is not evaluation evidence.
    if !matches!(
        record.evidential_status,
        EvidentialStatus::Assumption | EvidentialStatus::Simulation
    ) {
        record.evidential_status = EvidentialStatus::AttributedStatement;
    }
    record.qualification = Qualification::Candidate;
    Ok(())
}
fn text(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 8000 {
        return Err(Error::Invalid("Text must contain 1 to 8000 bytes".into()));
    }
    Ok(())
}
async fn save_exploration(
    c: &mut AnyConnection,
    auth: &Authority,
    value: &mut Exploration,
) -> Result<()> {
    if serde_json::to_vec(value)?.len() > 512 * 1024 {
        return Err(Error::LimitExceeded);
    }
    let expected = value.revision;
    value.revision = value.revision.checked_add(1).ok_or(Error::LimitExceeded)?;
    changed(sqlx::query("UPDATE explorations SET revision=$1,data=$2 WHERE tenant_id=$3 AND id=$4 AND revision=$5").bind(i64::from(value.revision)).bind(serde_json::to_string(value)?).bind(auth.tenant_id.to_string()).bind(value.id.to_string()).bind(i64::from(expected)).execute(c).await?.rows_affected())
}
