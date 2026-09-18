use crate::{
    Host,
    maintenance::{family, fields},
};
use memory_domain::{contracts::*, coordination::*, intentions::*, records::*};
use memory_store::{Error, Result};
use std::collections::BTreeMap;
use uuid::Uuid;

impl Host {
    async fn intention_access(
        &self,
        a: &Authority,
        f: &Fence,
        id: Uuid,
    ) -> Result<IntentionOccurrence> {
        let job = self.store.assigned_job(a, f).await?;
        let o = self.store.intention(a, id).await?;
        if job.cancel_requested
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
        if !matches!(
            job.spec.brief.process,
            Process::Maintenance | Process::Activation
        ) {
            return Err(Error::Forbidden);
        }
        Ok(o)
    }
    pub(crate) async fn intention_check(
        &self,
        a: &Authority,
        f: &Fence,
        id: Uuid,
        occurrence_id: Uuid,
        kind: IntentionCheckKind,
    ) -> Result<IntentionCheck> {
        let scoped = self.maintenance_scope(a, f).await?;
        let a = &scoped;
        if let Ok(prior) = self.store.load_intention_check(a, f, id).await {
            if prior.occurrence.id != occurrence_id
                || serde_json::to_value(&prior.kind)? != serde_json::to_value(&kind)?
            {
                return Err(Error::Conflict);
            }
            self.publish_work_artifact(
                a,
                f,
                id,
                "Intention transition evidence".into(),
                serde_json::to_string(&prior)?,
                vec![],
            )
            .await?;
            return Ok(prior);
        }
        let o = self.intention_access(a, f, occurrence_id).await?;
        let plan = o.plan().ok_or(Error::Invalid(
            "Intention needs a structured execution plan".into(),
        ))?;
        let mut event = None;
        let evidence = match &kind {
            IntentionCheckKind::Readiness => {
                let mut memories = vec![];
                for r in &plan.ready_memories {
                    memories.push(self.store.current_consolidation_memory(a, r).await?);
                }
                let mut artifacts = vec![];
                for id in &plan.ready_artifacts {
                    let artifact = self.artifacts.inspect(a, *id).await?;
                    if artifact.spec.expected_bytes > 16384 {
                        return Err(Error::LimitExceeded);
                    }
                    let bytes = self
                        .artifacts
                        .read(a, *id, 0..artifact.spec.expected_bytes)
                        .await?;
                    artifacts
                        .push(serde_json::json!({"id":id,"text":String::from_utf8_lossy(&bytes)}));
                }
                serde_json::json!({"memories":memories,"artifacts":artifacts})
            }
            IntentionCheckKind::Trigger { event_id } => {
                event = Some(self.store.intention_event(a, *event_id).await?);
                serde_json::Value::Null
            }
            IntentionCheckKind::Completion => serde_json::to_value(
                self.store
                    .job(a, o.job_id.ok_or(Error::Unavailable)?)
                    .await?
                    .result
                    .ok_or(Error::Unavailable)?,
            )?,
        };
        let check = IntentionCheck {
            id,
            occurrence: o,
            kind,
            event,
            evidence,
        };
        self.store.save_intention_check(a, f, &check).await?;
        self.publish_work_artifact(
            a,
            f,
            id,
            "Intention transition evidence".into(),
            serde_json::to_string(&check)?,
            vec![],
        )
        .await?;
        Ok(check)
    }
    pub(crate) async fn apply_intention_check(
        &self,
        a: &Authority,
        f: &Fence,
        id: Uuid,
        decisions: BTreeMap<String, Uuid>,
    ) -> Result<IntentionOccurrence> {
        let check = self.store.load_intention_check(a, f, id).await?;
        let current = self.intention_access(a, f, check.occurrence.id).await?;
        if current.definition.reference != check.occurrence.definition.reference
            || current.job_id != check.occurrence.job_id
        {
            return Err(Error::Conflict);
        }
        let content = serde_json::to_value(&check)?;
        let plan = current.plan().ok_or(Error::Unavailable)?;
        match check.kind {
            IntentionCheckKind::Readiness => {
                if self.store.assigned_job(a, f).await?.spec.brief.process != Process::Maintenance {
                    return Err(Error::Forbidden);
                }
                let readiness = match &current.definition.record.content {
                    MemoryContent::Intention { readiness, .. } => readiness,
                    _ => return Err(Error::Forbidden),
                };
                let mut ready = true;
                for i in 0..readiness.len() {
                    let choice = self
                        .checked_choice(
                            a,
                            f,
                            (id, &content),
                            decisions.get(&format!("J08/{i}")),
                            &family("J08")?,
                            fields(&[
                                ("method", "/occurrence/definition/record/content"),
                                (
                                    "prerequisites",
                                    &format!("/occurrence/definition/record/content/readiness/{i}"),
                                ),
                                ("evidence", "/evidence"),
                            ]),
                        )
                        .await?;
                    ready &= choice.as_deref() == Some("established");
                }
                self.store
                    .arm_intention(a, current.id, ready, Some(f))
                    .await
            }
            IntentionCheckKind::Trigger { event_id } => {
                if self.store.assigned_job(a, f).await?.spec.brief.process != Process::Activation {
                    return Err(Error::Forbidden);
                }
                let matched = if let IntentionTrigger::Semantic {
                    definition,
                    matched_choice,
                } = &plan.trigger
                {
                    let policy = self
                        .store
                        .policy(a, &current.definition.record.decision.policy)
                        .await?;
                    if !policy.policy.semantic_triggers.iter().any(|d| {
                        serde_json::to_value(d).ok() == serde_json::to_value(definition).ok()
                    }) {
                        return Err(Error::Forbidden);
                    }
                    let choice = self
                        .checked_choice(
                            a,
                            f,
                            (id, &content),
                            decisions.get("trigger"),
                            definition,
                            fields(&[("intention", "/occurrence/definition"), ("event", "/event")]),
                        )
                        .await?;
                    if choice.is_none() || choice.as_deref() == Some("insufficient") {
                        return Ok(current);
                    }
                    choice.as_deref() == Some(matched_choice.as_str())
                } else {
                    false
                };
                self.store
                    .fire_intention(a, current.id, Some(event_id), matched, Some(f))
                    .await
            }
            IntentionCheckKind::Completion => {
                if self.store.assigned_job(a, f).await?.spec.brief.process != Process::Maintenance {
                    return Err(Error::Forbidden);
                }
                let mut satisfied = true;
                for i in 0..plan.completion.semantic_conditions.len() {
                    let choice=self.checked_choice(a,f,(id,&content),decisions.get(&format!("J17/{i}")),&family("J17")?,fields(&[("completion_condition",&format!("/occurrence/definition/record/content/plan/completion/semantic_conditions/{i}")),("evidence","/evidence")])).await?;
                    satisfied &= choice.as_deref() == Some("satisfied");
                }
                self.store
                    .complete_intention(a, current.id, satisfied, Some(f))
                    .await
            }
        }
    }
}
