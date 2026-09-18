use crate::{
    Error, Result, Store,
    access::{read_scope, require_write_scope, write_scope},
    coordinator::{events::append_event, job, jobs::submit},
    database::{id, tag, timestamp},
    memories::memory_version,
};
use chrono::Duration;
use memory_domain::{contracts::*, coordination::*, intentions::*, records::*};
use sqlx::{AnyConnection, Row};
use uuid::Uuid;

pub(crate) async fn occurrence(
    c: &mut AnyConnection,
    a: &Authority,
    id: Uuid,
) -> Result<IntentionOccurrence> {
    read_scope(c, a, id).await?;
    let row = sqlx::query("SELECT data FROM intention_occurrences WHERE tenant_id=$1 AND id=$2")
        .bind(a.tenant_id.to_string())
        .bind(id.to_string())
        .fetch_optional(&mut *c)
        .await?
        .ok_or(Error::NotFound)?;
    let o: IntentionOccurrence = serde_json::from_str(row.get("data"))?;
    read_scope(c, a, o.definition.reference.memory_id).await?;
    Ok(o)
}
async fn save(
    c: &mut AnyConnection,
    a: &Authority,
    o: &IntentionOccurrence,
    at: CommitPosition,
) -> Result<()> {
    let row = sqlx::query("SELECT data FROM intention_occurrences WHERE tenant_id=$1 AND id=$2")
        .bind(a.tenant_id.to_string())
        .bind(o.id.to_string())
        .fetch_one(&mut *c)
        .await?;
    let previous: IntentionOccurrence = serde_json::from_str(row.get("data"))?;
    let recurring = o.state != IntentionState::Cancelled
        && o.plan().is_some_and(|p| p.recurrence_seconds.is_some())
        && matches!(&o.definition.record.content, MemoryContent::Intention {expires_at:Some(h),..} if *h > at.recorded_at);
    sqlx::query("UPDATE intention_occurrences SET recurring=$7,state=$1,due_at=$2,expires_at=$3,data=$4 WHERE tenant_id=$5 AND id=$6")
      .bind(tag(&o.state)?).bind(o.due_at.map(|d|d.timestamp_millis())).bind(o.terminal_due().map(|d|d.timestamp_millis())).bind(serde_json::to_string(o)?).bind(a.tenant_id.to_string()).bind(o.id.to_string()).bind(i64::from(recurring)).execute(&mut *c).await?;
    if previous.state != o.state
        || previous.evidence != o.evidence
        || previous.confirmed_by != o.confirmed_by
    {
        append_event(
            c,
            a,
            o.id,
            &format!("intention_{}", tag(&o.state)?),
            at,
            o.terminal_due().unwrap_or(at.recorded_at) + Duration::days(30),
        )
        .await?;
    }
    Ok(())
}
async fn insert(
    c: &mut AnyConnection,
    a: &Authority,
    o: &IntentionOccurrence,
    at: CommitPosition,
) -> Result<()> {
    write_scope(c, a, o.id, &o.definition.record.scope).await?;
    sqlx::query("INSERT INTO intention_occurrences(tenant_id,id,definition_id,revision,cycle,state,data) VALUES($1,$2,$3,$4,$5,$6,$7)")
    .bind(a.tenant_id.to_string()).bind(o.id.to_string()).bind(o.definition.reference.memory_id.to_string()).bind(i64::from(o.definition.reference.revision.get())).bind(i64::from(o.cycle)).bind(tag(&o.state)?).bind(serde_json::to_string(o)?).execute(&mut *c).await?;
    save(c, a, o, at).await?;
    append_event(
        c,
        a,
        o.id,
        "intention_pending",
        at,
        o.terminal_due().unwrap_or(at.recorded_at) + Duration::days(30),
    )
    .await
}
fn new_occurrence(
    definition: MemoryVersion,
    cycle: u32,
    previous: Option<Uuid>,
    at: CommitPosition,
) -> IntentionOccurrence {
    let (mut due, mut expires) = (None, None);
    if let MemoryContent::Intention {
        expires_at, plan, ..
    } = &definition.record.content
    {
        expires = *expires_at;
        if let Some(p) = plan {
            if let IntentionTrigger::Time { at } = &p.trigger {
                due = Some(
                    *at + Duration::seconds(
                        i64::from(cycle) * i64::from(p.recurrence_seconds.unwrap_or(0)),
                    ),
                );
                if let Some(interval) = p.recurrence_seconds {
                    expires = expires
                        .map(|h| h.min(due.unwrap() + Duration::seconds(i64::from(interval))));
                }
            }
        }
    }
    IntentionOccurrence {
        id: Uuid::now_v7(),
        definition,
        cycle,
        previous,
        state: IntentionState::Pending,
        event_cursor: at.sequence,
        due_at: due,
        expires_at: expires,
        job_id: None,
        trigger_event: None,
        confirmed_by: None,
        reason: None,
        evidence: vec![],
        unresolved: vec![],
    }
}
/// Called in the memory write transaction. A fired definition must be cancelled before amendment.
pub(crate) async fn sync_definition(
    c: &mut AnyConnection,
    a: &Authority,
    record: &MemoryVersion,
    at: CommitPosition,
) -> Result<()> {
    if !matches!(record.record.content, MemoryContent::Intention { .. }) {
        return Ok(());
    }
    let rows=sqlx::query("SELECT data FROM intention_occurrences WHERE tenant_id=$1 AND definition_id=$2 ORDER BY revision DESC,cycle DESC LIMIT 1")
      .bind(a.tenant_id.to_string()).bind(record.reference.memory_id.to_string()).fetch_all(&mut *c).await?;
    let mut previous = None;
    if let Some(row) = rows.first() {
        let mut prior: IntentionOccurrence = serde_json::from_str(row.get("data"))?;
        if prior.state == IntentionState::Fired {
            return Err(Error::Invalid(
                "Cancel the fired occurrence before changing its definition".into(),
            ));
        }
        previous = Some(prior.id);
        if !prior.state.terminal() {
            prior.state = IntentionState::Cancelled;
            prior.reason = Some("Definition amended".into());
            save(c, a, &prior, at).await?;
        }
    }
    insert(c, a, &new_occurrence(record.clone(), 0, previous, at), at).await
}
pub(crate) async fn check_plan(
    c: &mut AnyConnection,
    a: &Authority,
    job: &Job,
    record: &RecordDraft,
) -> Result<()> {
    if let MemoryContent::Intention { plan: Some(p), .. } = &record.content {
        let mut required = (*p.execution).clone();
        required.inputs.memories.extend(p.ready_memories.clone());
        required.inputs.artifacts.extend(p.ready_artifacts.clone());
        crate::coordinator::scoped::check_child(c, a, job, &required).await?;
    }
    Ok(())
}
async fn ready(c: &mut AnyConnection, a: &Authority, o: &IntentionOccurrence) -> Result<()> {
    if !matches!(&o.definition.record.content, MemoryContent::Intention {owner_id,trigger,completion,..}
        if !owner_id.is_nil() && !trigger.trim().is_empty() && !completion.is_empty())
    {
        return Err(Error::Invalid(
            "Owner, trigger and completion are required".into(),
        ));
    }
    let plan = o
        .plan()
        .ok_or(Error::Invalid("Intention has no executable plan".into()))?;
    let expires = o
        .expires_at
        .ok_or(Error::Invalid("Intention needs a finite expiry".into()))?;
    plan.validate(expires).map_err(Error::Invalid)?;
    crate::policies::current_policy(c, a, &o.definition.record.decision.policy).await?;
    let current = memory_version(c, a, &o.definition.reference).await?;
    if current.recorded_until.is_some() || current.record.availability != Availability::Routine {
        return Err(Error::Conflict);
    }
    if !o.definition.record.scope.permits(&plan.execution.scope)
        || !a.scope.permits(&plan.execution.scope)
    {
        return Err(Error::Forbidden);
    }
    for r in &plan.ready_memories {
        let m = memory_version(c, a, r).await?;
        if m.recorded_until.is_some() || m.record.availability != Availability::Routine {
            return Err(Error::Unavailable);
        }
    }
    for id in &plan.ready_artifacts {
        crate::artifacts::ready_artifact(c, a, *id).await?;
    }
    Ok(())
}
impl Store {
    pub async fn intention(&self, a: &Authority, id: Uuid) -> Result<IntentionOccurrence> {
        occurrence(&mut *self.pool.acquire().await?, a, id).await
    }
    pub async fn intentions(
        &self,
        a: &Authority,
        definition_id: Uuid,
    ) -> Result<Vec<IntentionOccurrence>> {
        let mut c = self.pool.acquire().await?;
        read_scope(&mut c, a, definition_id).await?;
        let rows=sqlx::query("SELECT data FROM intention_occurrences WHERE tenant_id=$1 AND definition_id=$2 ORDER BY revision,cycle LIMIT 100")
          .bind(a.tenant_id.to_string()).bind(definition_id.to_string()).fetch_all(&mut *c).await?;
        rows.iter()
            .map(|r| serde_json::from_str(r.get("data")).map_err(Error::from))
            .collect()
    }
    pub async fn arm_intention(
        &self,
        a: &Authority,
        id: Uuid,
        readiness_checked: bool,
        permit: Option<&Fence>,
    ) -> Result<IntentionOccurrence> {
        let (mut tx, at) = self.begin_write().await?;
        require_write_scope(&mut tx, a, id).await?;
        let mut o = occurrence(&mut tx, a, id).await?;
        transition_authority(&mut tx, a, permit, &o, Process::Maintenance, at).await?;
        if o.state != IntentionState::Pending {
            return Ok(o);
        }
        if o.expires_at.is_some_and(|e| e <= at.recorded_at) {
            o.state = IntentionState::Expired;
            o.reason = Some("Expired before arming".into());
        } else {
            ready(&mut tx, a, &o).await?;
            if let MemoryContent::Intention {
                owner_id,
                trigger,
                completion,
                readiness,
                ..
            } = &o.definition.record.content
            {
                if owner_id.is_nil()
                    || trigger.trim().is_empty()
                    || completion.is_empty()
                    || (!readiness.is_empty() && !readiness_checked)
                {
                    return Err(Error::Invalid(
                        "Owner, completion and checked readiness are required".into(),
                    ));
                }
            }
            o.state = IntentionState::Armed;
        }
        // Preserve the creation cursor so events arriving during setup remain eligible.
        save(&mut tx, a, &o, at).await?;
        tx.commit().await?;
        Ok(o)
    }
    pub async fn intention_event(&self, a: &Authority, event_id: Uuid) -> Result<Event> {
        event(&mut *self.pool.acquire().await?, a, event_id).await
    }
    pub async fn fire_intention(
        &self,
        a: &Authority,
        id: Uuid,
        event_id: Option<Uuid>,
        semantic_match: bool,
        permit: Option<&Fence>,
    ) -> Result<IntentionOccurrence> {
        let (mut tx, at) = self.begin_write().await?;
        require_write_scope(&mut tx, a, id).await?;
        let mut o = occurrence(&mut tx, a, id).await?;
        transition_authority(&mut tx, a, permit, &o, Process::Activation, at).await?;
        if o.state != IntentionState::Armed {
            return Ok(o);
        }
        if o.expires_at.is_none_or(|e| e <= at.recorded_at) {
            o.state = IntentionState::Expired;
            o.reason = Some("Trigger arrived after expiry".into());
            save(&mut tx, a, &o, at).await?;
            tx.commit().await?;
            return Ok(o);
        }
        ready(&mut tx, a, &o).await?;
        let plan = o.plan().unwrap().clone();
        let e = match event_id {
            Some(id) => Some(
                event(
                    &mut tx,
                    &Authority {
                        scope: o.definition.record.scope.clone(),
                        ..a.clone()
                    },
                    id,
                )
                .await?,
            ),
            None => None,
        };
        if e.as_ref().is_some_and(|e| e.cursor <= o.event_cursor) {
            return Ok(o);
        }
        let matches = match &plan.trigger {
            IntentionTrigger::Time { .. } => o.due_at.is_some_and(|d| d <= at.recorded_at),
            IntentionTrigger::Event {
                resource_id,
                event_kind,
            } => e
                .as_ref()
                .is_some_and(|e| e.resource_id == *resource_id && e.kind == *event_kind),
            IntentionTrigger::Result { job_id } => {
                if let Some(e) = &e {
                    e.resource_id == *job_id
                        && e.kind == "job_settled"
                        && job(&mut tx, a, *job_id).await?.state == JobState::Completed
                } else {
                    false
                }
            }
            IntentionTrigger::SourceRevision {
                source_id,
                after_revision,
            } => {
                if let Some(e) = &e {
                    sqlx::query("SELECT revision FROM source_versions WHERE tenant_id=$1 AND source_id=$2 AND scope_id=$3 AND revision<>$4")
                  .bind(a.tenant_id.to_string()).bind(source_id.to_string()).bind(e.resource_id.to_string()).bind(after_revision).fetch_optional(&mut *tx).await?.is_some() && e.kind=="source_registered"
                } else {
                    false
                }
            }
            IntentionTrigger::Semantic { .. } => e.is_some() && semantic_match,
        };
        if let Some(e) = &e {
            o.event_cursor = e.cursor;
        }
        if matches {
            let deadline = o.expires_at.unwrap()
                + Duration::seconds(i64::from(
                    plan.completion.delivery_grace_seconds.unwrap_or(0),
                ));
            let command = Command {
                command_version: WireVersion::V1,
                request_id: Uuid::now_v7(),
                tenant_id: a.tenant_id,
                actor_id: a.actor_id,
                scope: plan.execution.scope.clone(),
                job_id: None,
                session_id: None,
                operation_id: None,
                lane_id: None,
                invocation_id: None,
                expected_revisions: vec![],
                deadline,
                budget_id: plan.execution.limits.root_budget_id,
                lease_epoch: None,
                payload: SubmitJob {
                    brief: *plan.execution,
                    parent_id: None,
                    max_attempts: 3,
                    retain_until: deadline + Duration::days(30),
                },
            };
            let j = submit(&mut tx, a, &command, at, false).await?;
            o.state = IntentionState::Fired;
            o.unresolved.clear();
            o.job_id = Some(j.id);
            o.trigger_event = event_id;
        }
        save(&mut tx, a, &o, at).await?;
        tx.commit().await?;
        Ok(o)
    }
    pub async fn confirm_intention(&self, a: &Authority, id: Uuid) -> Result<IntentionOccurrence> {
        let (mut tx, at) = self.begin_write().await?;
        let mut o = occurrence(&mut tx, a, id).await?;
        if o.plan().and_then(|p| p.completion.confirmation_owner) != Some(a.actor_id) {
            return Err(Error::Forbidden);
        }
        if o.state != IntentionState::Fired && !o.state.terminal() {
            return Err(Error::Invalid(
                "Confirmation requires a fired occurrence".into(),
            ));
        }
        if !o.state.terminal() {
            o.confirmed_by = Some(a.actor_id);
            save(&mut tx, a, &o, at).await?;
            tx.commit().await?;
        }
        Ok(o)
    }
    pub async fn cancel_intention(
        &self,
        a: &Authority,
        id: Uuid,
        reason: String,
    ) -> Result<IntentionOccurrence> {
        if reason.trim().is_empty() {
            return Err(Error::Invalid("Cancellation needs a reason".into()));
        }
        let (mut tx, at) = self.begin_write().await?;
        require_write_scope(&mut tx, a, id).await?;
        let mut o = occurrence(&mut tx, a, id).await?;
        if !o.state.terminal() {
            o.state = IntentionState::Cancelled;
            o.reason = Some(reason);
            request_stop(&mut tx, a, &mut o).await?;
            save(&mut tx, a, &o, at).await?;
            tx.commit().await?;
        }
        Ok(o)
    }
    pub async fn complete_intention(
        &self,
        a: &Authority,
        id: Uuid,
        semantic_checked: bool,
        permit: Option<&Fence>,
    ) -> Result<IntentionOccurrence> {
        let (mut tx, at) = self.begin_write().await?;
        require_write_scope(&mut tx, a, id).await?;
        let mut o = occurrence(&mut tx, a, id).await?;
        transition_authority(&mut tx, a, permit, &o, Process::Maintenance, at).await?;
        let j = job(&mut tx, a, o.job_id.ok_or(Error::Unavailable)?).await?;
        let result = j.result.as_ref().ok_or(Error::Unavailable)?;
        let artifact = result.result_artifact.ok_or(Error::Unavailable)?;
        crate::artifacts::ready_artifact(&mut tx, a, artifact).await?;
        if !o.evidence.contains(&artifact) {
            o.evidence.push(artifact);
        }
        if o.state.terminal() {
            save(&mut tx, a, &o, at).await?;
            tx.commit().await?;
            return Ok(o);
        }
        let plan = o.plan().ok_or(Error::Unavailable)?.clone();
        let ended=sqlx::query("SELECT MAX(recorded_at) AS time FROM outbox_events WHERE tenant_id=$1 AND resource_id=$2 AND kind='job_settled'").bind(a.tenant_id.to_string()).bind(j.id.to_string()).fetch_one(&mut *tx).await?.try_get::<Option<i64>,_>("time")?.map(timestamp).transpose()?.ok_or(Error::Unavailable)?;
        o.unresolved.clear();
        if !o.accepts_completion(at.recorded_at, ended) {
            o.state = IntentionState::Expired;
            o.reason = Some("Checked completion missed its deadline".into());
            request_stop(&mut tx, a, &mut o).await?;
        } else {
            if j.state != JobState::Completed
                || j.cancel_requested
                || result.status != WorkStatus::Complete
                || !result.unresolved_work.is_empty()
                || !result.coverage.unexamined.is_empty()
            {
                o.unresolved.push("Execution result is incomplete".into());
            }
            if !result.examined_scope.permits(&plan.execution.scope)
                || !plan.execution.scope.permits(&result.examined_scope)
            {
                o.unresolved
                    .push("Completion scope differs from the declared work".into());
            }
            if !plan
                .execution
                .inputs
                .sources
                .iter()
                .all(|r| result.inputs.sources.contains(r))
                || !plan
                    .execution
                    .inputs
                    .memories
                    .iter()
                    .all(|r| result.inputs.memories.contains(r))
                || !plan
                    .execution
                    .inputs
                    .artifacts
                    .iter()
                    .all(|r| result.inputs.artifacts.contains(r))
            {
                o.unresolved
                    .push("Completion does not cover the required inputs".into());
            }
            if serde_json::to_value(result)?.pointer(&plan.completion.result_pointer)
                != Some(&plan.completion.equals)
            {
                o.unresolved
                    .push("Result predicate is not satisfied".into());
            }
            let unknown:i64=sqlx::query("SELECT COUNT(*) AS n FROM effect_requests e JOIN jobs j ON j.tenant_id=e.tenant_id AND j.id=e.job_id WHERE e.tenant_id=$1 AND j.root_id=$2 AND e.state IN ('prepared','in_progress','outcome_unknown')").bind(a.tenant_id.to_string()).bind(j.id.to_string()).fetch_one(&mut *tx).await?.get("n");
            if unknown > 0 {
                o.unresolved
                    .push("Execution effects remain unresolved".into());
            }
            for key in &plan.completion.required_effects {
                let operation = format!("{}/{key}", j.operation_id);
                let receipts=sqlx::query("SELECT state,receipt FROM effect_requests WHERE tenant_id=$1 AND job_id=$2 AND logical_operation_id=$3")
                    .bind(a.tenant_id.to_string()).bind(j.id.to_string()).bind(operation).fetch_all(&mut *tx).await?;
                if receipts.is_empty()
                    || !receipts.iter().all(|r| {
                        r.get::<String, _>("state") == "succeeded"
                            && r.get::<Option<String>, _>("receipt").is_some()
                    })
                {
                    o.unresolved
                        .push(format!("Required effect is not verified: {key}"));
                }
            }
            if !plan.completion.semantic_conditions.is_empty() && !semantic_checked {
                o.unresolved
                    .push("Semantic completion needs assessment".into());
            }
            if plan.completion.confirmation_owner.is_some()
                && o.confirmed_by != plan.completion.confirmation_owner
            {
                o.unresolved
                    .push("Designated confirmation is outstanding".into());
            }
            if o.unresolved.is_empty() {
                o.state = IntentionState::Completed;
                o.reason = Some("Declared completion checks passed".into());
            }
        }
        save(&mut tx, a, &o, at).await?;
        tx.commit().await?;
        Ok(o)
    }
}
async fn transition_authority(
    c: &mut AnyConnection,
    a: &Authority,
    permit: Option<&Fence>,
    o: &IntentionOccurrence,
    process: Process,
    at: CommitPosition,
) -> Result<()> {
    if let Some(permit) = permit {
        let job = crate::coordinator::fence(c, a, permit, at.recorded_at).await?;
        if job.cancel_requested
            || job.spec.brief.process != process
            || !job
                .spec
                .brief
                .inputs
                .memories
                .contains(&o.definition.reference)
            || !job.spec.brief.scope.permits(&o.definition.record.scope)
        {
            return Err(Error::Forbidden);
        }
        crate::policies::current_policy(c, a, &job.spec.brief.policy).await?;
    }
    Ok(())
}
async fn request_stop(
    c: &mut AnyConnection,
    a: &Authority,
    o: &mut IntentionOccurrence,
) -> Result<()> {
    if let Some(job) = o.job_id {
        sqlx::query("UPDATE jobs SET cancel_requested=1 WHERE tenant_id=$1 AND root_id=$2 AND state NOT IN ('completed','partial','failed','cancelled')").bind(a.tenant_id.to_string()).bind(job.to_string()).execute(&mut *c).await?;
        let n:i64=sqlx::query("SELECT COUNT(*) AS n FROM effect_requests e JOIN jobs j ON j.tenant_id=e.tenant_id AND j.id=e.job_id WHERE e.tenant_id=$1 AND j.root_id=$2 AND e.state IN ('in_progress','outcome_unknown')").bind(a.tenant_id.to_string()).bind(job.to_string()).fetch_one(&mut *c).await?.get("n");
        if n > 0 {
            o.unresolved
                .push("In-flight effects need reconciliation after stop".into());
        }
    }
    Ok(())
}
async fn event(c: &mut AnyConnection, a: &Authority, event_id: Uuid) -> Result<Event> {
    let r = sqlx::query("SELECT * FROM outbox_events WHERE tenant_id=$1 AND id=$2")
        .bind(a.tenant_id.to_string())
        .bind(event_id.to_string())
        .fetch_optional(&mut *c)
        .await?
        .ok_or(Error::NotFound)?;
    let resource_id = id(r.get("resource_id"))?;
    read_scope(c, a, resource_id).await?;
    Ok(Event {
        id: event_id,
        cursor: crate::database::number(r.get("cursor"))?,
        resource_id,
        kind: r.get("kind"),
        recorded_at: timestamp(r.get("recorded_at"))?,
    })
}

impl Store {
    async fn sweep_trigger(
        &self,
        a: &Authority,
        id: Uuid,
        event: Option<Uuid>,
    ) -> Result<IntentionOccurrence> {
        match self.fire_intention(a, id, event, false, None).await {
            Ok(o) => Ok(o),
            Err(
                Error::Conflict
                | Error::Unavailable
                | Error::NotFound
                | Error::Forbidden
                | Error::Invalid(_)
                | Error::LimitExceeded,
            ) => {
                let (mut tx, at) = self.begin_write().await?;
                let mut o = occurrence(&mut tx, a, id).await?;
                let reason = "Trigger blocked by current scope, evidence, plan or budget";
                if !o.unresolved.iter().any(|s| s == reason) {
                    o.unresolved.push(reason.into());
                }
                save(&mut tx, a, &o, at).await?;
                tx.commit().await?;
                Ok(o)
            }
            Err(e) => Err(e),
        }
    }
    /// A bounded durable timer/event pump. Call after downtime and on the host's normal polling cadence.
    pub async fn sweep_intentions(&self, a: &Authority, limit: u16) -> Result<IntentionSweep> {
        let limit = limit.clamp(1, 100);
        let (mut tx, at) = self.begin_write().await?;
        let (filter, values) = crate::access::predicate(a);
        let sql = format!(
            "SELECT o.data FROM intention_occurrences o JOIN resource_scopes s ON s.tenant_id=o.tenant_id AND s.id=o.id WHERE {filter} AND NOT EXISTS (SELECT 1 FROM intention_occurrences n WHERE n.tenant_id=o.tenant_id AND n.definition_id=o.definition_id AND (n.revision>o.revision OR (n.revision=o.revision AND n.cycle>o.cycle))) AND (o.state IN ('pending','armed','fired') OR o.recurring=1) ORDER BY o.polled_at,o.id LIMIT {}",
            limit + 1
        );
        let mut query = sqlx::query(&sql);
        for v in values {
            query = query.bind(v);
        }
        let rows = query.fetch_all(&mut *tx).await?;
        let more = rows.len() > usize::from(limit);
        let mut selected = vec![];
        for row in rows.iter().take(usize::from(limit)) {
            let mut o: IntentionOccurrence = serde_json::from_str(row.get("data"))?;
            if !a.scope.permits(&o.definition.record.scope) {
                continue;
            }
            if !o.state.terminal() && o.terminal_due().is_some_and(|due| due <= at.recorded_at) {
                o.state = IntentionState::Expired;
                o.reason = Some("Durable expiry timer elapsed".into());
                request_stop(&mut tx, a, &mut o).await?;
                save(&mut tx, a, &o, at).await?;
            }
            if o.state.terminal() {
                if let Some(interval) = o.plan().and_then(|p| p.recurrence_seconds) {
                    let horizon = match &o.definition.record.content {
                        MemoryContent::Intention { expires_at, .. } => *expires_at,
                        _ => None,
                    };
                    if horizon.is_some_and(|h| at.recorded_at < h) {
                        let base = match &o.plan().unwrap().trigger {
                            IntentionTrigger::Time { at } => *at,
                            _ => continue,
                        };
                        let elapsed = ((at.recorded_at - base).num_seconds().max(0)
                            / i64::from(interval)) as u32;
                        let cycle = o
                            .cycle
                            .checked_add(1)
                            .ok_or(Error::LimitExceeded)?
                            .max(elapsed);
                        let next = new_occurrence(o.definition.clone(), cycle, Some(o.id), at);
                        if next.due_at.zip(horizon).is_some_and(|(due, h)| due < h) {
                            insert(&mut tx, a, &next, at).await?;
                            o = next;
                        }
                    }
                }
            }
            if o.state == IntentionState::Pending
                && matches!(&o.definition.record.content,MemoryContent::Intention{readiness,..} if readiness.is_empty())
            {
                match ready(&mut tx, a, &o).await {
                    Ok(()) => {
                        o.state = IntentionState::Armed;
                        save(&mut tx, a, &o, at).await?;
                    }
                    Err(Error::Unavailable | Error::Conflict | Error::Invalid(_)) => {}
                    Err(e) => return Err(e),
                }
            }
            save(&mut tx, a, &o, at).await?;
            sqlx::query(
                "UPDATE intention_occurrences SET polled_at=$1 WHERE tenant_id=$2 AND id=$3",
            )
            .bind(i64::from(at.sequence))
            .bind(a.tenant_id.to_string())
            .bind(o.id.to_string())
            .execute(&mut *tx)
            .await?;
            selected.push(o);
        }
        tx.commit().await?;
        for o in &mut selected {
            if o.state != IntentionState::Armed {
                continue;
            }
            if matches!(
                o.plan().map(|p| &p.trigger),
                Some(IntentionTrigger::Time { .. })
            ) {
                *o = self.sweep_trigger(a, o.id, None).await?;
            } else {
                let scope = Authority {
                    scope: o.definition.record.scope.clone(),
                    ..a.clone()
                };
                let page = self.events(&scope, o.event_cursor, 100).await?;
                if page.snapshot_required {
                    // Source/result triggers can be reconstructed from current retained records.
                    let mut c = self.pool.acquire().await?;
                    let event_id=match o.plan().map(|p|&p.trigger){
                        Some(IntentionTrigger::Result{job_id})=>sqlx::query("SELECT id FROM outbox_events WHERE tenant_id=$1 AND resource_id=$2 AND kind='job_settled' ORDER BY cursor DESC LIMIT 1").bind(a.tenant_id.to_string()).bind(job_id.to_string()).fetch_optional(&mut *c).await?,
                        Some(IntentionTrigger::SourceRevision{source_id,after_revision})=>sqlx::query("SELECT e.id FROM outbox_events e JOIN source_versions v ON v.tenant_id=e.tenant_id AND v.scope_id=e.resource_id WHERE e.tenant_id=$1 AND v.source_id=$2 AND v.revision<>$3 AND e.kind='source_registered' ORDER BY e.cursor DESC LIMIT 1").bind(a.tenant_id.to_string()).bind(source_id.to_string()).bind(after_revision).fetch_optional(&mut *c).await?,
                        _=>None,
                    }.map(|r|id(r.get("id"))).transpose()?;
                    drop(c);
                    if let Some(id) = event_id {
                        *o = self.sweep_trigger(a, o.id, Some(id)).await?;
                    } else {
                        let (mut tx, at) = self.begin_write().await?;
                        let mut current = occurrence(&mut tx, a, o.id).await?;
                        if !current
                            .unresolved
                            .iter()
                            .any(|s| s == "Event history gap needs current-state investigation")
                        {
                            current
                                .unresolved
                                .push("Event history gap needs current-state investigation".into());
                            save(&mut tx, a, &current, at).await?;
                        }
                        tx.commit().await?;
                        *o = current;
                    }
                } else {
                    for event in page.events {
                        if matches!(
                            o.plan().map(|p| &p.trigger),
                            Some(IntentionTrigger::Semantic { .. })
                        ) {
                            break;
                        }
                        *o = self.sweep_trigger(a, o.id, Some(event.id)).await?;
                        if o.state != IntentionState::Armed {
                            break;
                        }
                    }
                }
            }
        }
        Ok(IntentionSweep {
            occurrences: selected,
            more,
        })
    }
}

impl Store {
    pub async fn save_intention_check(
        &self,
        a: &Authority,
        permit: &Fence,
        check: &IntentionCheck,
    ) -> Result<()> {
        self.assigned_job(a, permit).await?;
        if let Some(row) =
            sqlx::query("SELECT data,job_id FROM intention_checks WHERE tenant_id=$1 AND id=$2")
                .bind(a.tenant_id.to_string())
                .bind(check.id.to_string())
                .fetch_optional(&self.pool)
                .await?
        {
            if row.get::<String, _>("job_id") != permit.job_id.to_string()
                || serde_json::from_str::<serde_json::Value>(row.get("data"))?
                    != serde_json::to_value(check)?
            {
                return Err(Error::Conflict);
            }
            return Ok(());
        }
        sqlx::query("INSERT INTO intention_checks(tenant_id,id,job_id,data) VALUES($1,$2,$3,$4)")
            .bind(a.tenant_id.to_string())
            .bind(check.id.to_string())
            .bind(permit.job_id.to_string())
            .bind(serde_json::to_string(check)?)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    pub async fn load_intention_check(
        &self,
        a: &Authority,
        permit: &Fence,
        id: Uuid,
    ) -> Result<IntentionCheck> {
        self.assigned_job(a, permit).await?;
        let row = sqlx::query(
            "SELECT data FROM intention_checks WHERE tenant_id=$1 AND id=$2 AND job_id=$3",
        )
        .bind(a.tenant_id.to_string())
        .bind(id.to_string())
        .bind(permit.job_id.to_string())
        .fetch_optional(&self.pool)
        .await?
        .ok_or(Error::NotFound)?;
        Ok(serde_json::from_str(row.get("data"))?)
    }
}
