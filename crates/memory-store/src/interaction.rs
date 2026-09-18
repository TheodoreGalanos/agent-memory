//! Scoped user operations. Mutations and their retry receipts commit together.
mod mutations;
use crate::{
    Error, Result, Store,
    access::read_scope,
    database::{from_tag, id, tag},
};
use memory_domain::{
    contracts::*, coordination::*, interaction::*, records::*, workspace::RenderManifest,
};
use sqlx::{AnyConnection, Row};
use uuid::Uuid;

impl Store {
    pub async fn user_request(
        &self,
        auth: &Authority,
        request: UserRequest,
    ) -> Result<UserResponse> {
        use UserRequest as R;
        use UserResponse as O;
        Ok(match request {
            R::Identity => O::Identity {
                tenant_id: auth.tenant_id,
                actor_id: auth.actor_id,
                scope: auth.scope.clone(),
            },
            R::Browse { mut query } => {
                let limit = query.limit.clamp(1, 100);
                query.limit = limit + 1;
                let mut records = self.memories(auth, &query).await?;
                let next = if records.len() > usize::from(limit) {
                    records.truncate(limit.into());
                    records.last().map(|r| r.reference.memory_id)
                } else {
                    None
                };
                O::Records { records, next }
            }
            R::History {
                memory_id,
                after_revision,
                limit,
            } => {
                let mut c = self.pool.acquire().await?;
                read_scope(&mut c, auth, memory_id).await?;
                let rows = sqlx::query("SELECT revision,data FROM memory_versions WHERE tenant_id=$1 AND id=$2 AND revision>$3 ORDER BY revision LIMIT $4")
                    .bind(auth.tenant_id.to_string()).bind(memory_id.to_string()).bind(i64::from(after_revision)).bind(i64::from(limit.clamp(1,100))+1).fetch_all(&mut *c).await?;
                let more = rows.len() > usize::from(limit.clamp(1, 100));
                let mut records = vec![];
                for row in rows.into_iter().take(limit.clamp(1, 100).into()) {
                    let record: RecordDraft = serde_json::from_str(row.try_get("data")?)?;
                    let revision = crate::database::number(row.try_get("revision")?)?
                        .try_into()
                        .map_err(|_| Error::Conflict)?;
                    records.push(
                        crate::memories::memory_version(
                            &mut c,
                            auth,
                            &MemoryRef {
                                memory_id,
                                revision,
                                label: record.label,
                            },
                        )
                        .await?,
                    );
                }
                let next_revision = if more {
                    records.last().map(|r| r.reference.revision.get())
                } else {
                    None
                };
                O::History {
                    records,
                    next_revision,
                }
            }
            R::InspectMemory { reference } => O::Memory {
                memory: Box::new(self.memory(auth, &reference).await?),
                relations: self
                    .relations(auth, reference.memory_id, &MemoryQuery::default())
                    .await?,
            },
            R::Policy { reference } => O::Policy {
                policy: Box::new(self.policy(auth, &reference).await?),
            },
            R::Changes { after, limit } => {
                let page = self.events(auth, after, limit).await?;
                let mut revisions = vec![];
                for event in &page.events {
                    if let Some(row) = sqlx::query(
                        "SELECT data FROM memory_changes WHERE tenant_id=$1 AND cursor=$2",
                    )
                    .bind(auth.tenant_id.to_string())
                    .bind(i64::from(event.cursor))
                    .fetch_optional(&self.pool)
                    .await?
                    {
                        revisions.push(serde_json::from_str(row.try_get("data")?)?);
                    }
                }
                O::Changes { page, revisions }
            }
            R::InspectExploration { id } => O::Exploration {
                exploration: Box::new(
                    exploration(&mut *self.pool.acquire().await?, auth, id).await?,
                ),
            },
            R::Decisions { job_id } => O::Decisions {
                decisions: self.decisions(auth, job_id).await?,
            },
            R::InspectTask {
                job_id,
                after_manifest,
            } => {
                let job = self.job(auth, job_id).await?;
                let decisions = self.decisions(auth, job_id).await?;
                let rows = sqlx::query("SELECT data FROM task_contexts WHERE tenant_id=$1 AND job_id=$2 AND id>$3 ORDER BY id LIMIT 4").bind(auth.tenant_id.to_string()).bind(job_id.to_string()).bind(after_manifest.map(|id|id.to_string()).unwrap_or_default()).fetch_all(&self.pool).await?;
                let mut manifests: Vec<RenderManifest> = rows
                    .iter()
                    .map(|r| Ok(serde_json::from_str(r.try_get("data")?)?))
                    .collect::<Result<_>>()?;
                let more = manifests.len() > 3;
                manifests.truncate(3);
                let next_manifest = if more {
                    manifests.last().map(|m| m.decision_id)
                } else {
                    None
                };
                let coverage = Coverage {
                    examined: vec!["Stored job, results and decision requests".into()],
                    unexamined: vec![if manifests.is_empty() {
                        "No render manifests on this page; model context cannot be inferred".into()
                    } else {
                        "Up to three manifests per page; selection does not prove model use".into()
                    }],
                };
                O::Task {
                    job: Box::new(job),
                    manifests,
                    next_manifest,
                    decisions,
                    coverage,
                }
            }
            R::Notifications { after, limit } => {
                let mut page = self.events(auth, after, limit).await?;
                let row = sqlx::query(
                    "SELECT mode FROM notification_preferences WHERE tenant_id=$1 AND actor_id=$2",
                )
                .bind(auth.tenant_id.to_string())
                .bind(auth.actor_id.to_string())
                .fetch_optional(&self.pool)
                .await?;
                let mode = row
                    .map(|r| from_tag(r.try_get("mode")?))
                    .transpose()?
                    .unwrap_or(NotificationMode::Material);
                let mut retained = vec![];
                for event in page.events {
                    let settled = if event.kind == "job_settled" {
                        Some(self.job(auth, event.resource_id).await?.state)
                    } else {
                        None
                    };
                    let completion =
                        event.kind == "intention_completed" || settled == Some(JobState::Completed);
                    let blocker =
                        matches!(
                            event.kind.as_str(),
                            "decision_needed" | "intention_expired" | "job_waiting"
                        ) || matches!(settled, Some(JobState::Failed | JobState::Partial));
                    let relevant = match mode {
                        NotificationMode::Material => {
                            completion
                                || blocker
                                || matches!(
                                    event.kind.as_str(),
                                    "memory_maintained"
                                        | "user_memory_changed"
                                        | "exploration_promoted"
                                        | "decision_answered"
                                        | "job_cancelled"
                                        | "intention_cancelled"
                                        | "intention_pending"
                                )
                        }
                        NotificationMode::Muted => false,
                        NotificationMode::Blockers => blocker,
                        NotificationMode::Completion => completion,
                    };
                    if relevant && sqlx::query("SELECT event_id FROM notification_deliveries WHERE tenant_id=$1 AND actor_id=$2 AND event_id=$3").bind(auth.tenant_id.to_string()).bind(auth.actor_id.to_string()).bind(event.id.to_string()).fetch_optional(&self.pool).await?.is_none() { retained.push(event); }
                }
                page.events = retained;
                O::Notifications { page, mode }
            }
            R::Controls => {
                let rows=sqlx::query("SELECT * FROM runtime_controls WHERE tenant_id=$1 ORDER BY kind,target LIMIT 1000").bind(auth.tenant_id.to_string()).fetch_all(&self.pool).await?;
                O::Controls {
                    controls: rows
                        .iter()
                        .map(|r| {
                            Ok(RuntimeControl {
                                kind: from_tag(r.try_get("kind")?)?,
                                target: r.try_get("target")?,
                                revision: crate::database::number(r.try_get("revision")?)?,
                                enabled: r.try_get::<i64, _>("enabled")? != 0,
                            })
                        })
                        .collect::<Result<_>>()?,
                }
            }
            R::Mutate {
                request_id,
                mutation,
            } => self.user_mutation(auth, request_id, *mutation).await?,
        })
    }
    pub async fn decisions(&self, auth: &Authority, job_id: Uuid) -> Result<Vec<DecisionRequest>> {
        self.job(auth, job_id).await?;
        let rows=sqlx::query("SELECT data FROM decision_requests WHERE tenant_id=$1 AND job_id=$2 ORDER BY id LIMIT 100").bind(auth.tenant_id.to_string()).bind(job_id.to_string()).fetch_all(&self.pool).await?;
        rows.iter()
            .map(|r| Ok(serde_json::from_str(r.try_get("data")?)?))
            .collect()
    }
    pub async fn record_context(
        &self,
        auth: &Authority,
        permit: &Fence,
        manifest: RenderManifest,
    ) -> Result<()> {
        let (mut tx, pos) = self.begin_write().await?;
        let job = crate::coordinator::fence(&mut tx, auth, permit, pos.recorded_at).await?;
        if manifest.session_id != job.session_id
            || manifest.operation_id != job.operation_id
            || manifest.profile != job.spec.brief.profile
        {
            return Err(Error::Forbidden);
        }
        let data = serde_json::to_string(&manifest)?;
        if data.len() > 512 * 1024 {
            return Err(Error::LimitExceeded);
        }
        let prior = sqlx::query("SELECT job_id FROM task_contexts WHERE tenant_id=$1 AND id=$2")
            .bind(auth.tenant_id.to_string())
            .bind(manifest.decision_id.to_string())
            .fetch_optional(&mut *tx)
            .await?;
        if let Some(prior) = prior {
            if prior.try_get::<String, _>("job_id")? != job.id.to_string() {
                return Err(Error::Conflict);
            }
            crate::access::require_write_scope(&mut tx, auth, manifest.decision_id).await?;
        } else {
            crate::access::write_scope(&mut tx, auth, manifest.decision_id, &job.spec.brief.scope)
                .await?;
        }
        sqlx::query("INSERT INTO task_contexts (tenant_id,id,job_id,data) VALUES ($1,$2,$3,$4) ON CONFLICT (tenant_id,id) DO UPDATE SET data=excluded.data WHERE task_contexts.job_id=excluded.job_id").bind(auth.tenant_id.to_string()).bind(manifest.decision_id.to_string()).bind(job.id.to_string()).bind(data).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn check_execution(
        &self,
        auth: &Authority,
        permit: &Fence,
        provider: Option<&str>,
        family: Option<&str>,
    ) -> Result<()> {
        let (mut tx, pos) = self.begin_write().await?;
        let job = crate::coordinator::fence(&mut tx, auth, permit, pos.recorded_at).await?;
        execution_gate(&mut tx, auth, &job).await?;
        if let Some(provider) = provider {
            control_enabled(&mut tx, auth, ControlKind::Provider, provider).await?;
        }
        if let Some(family) = family {
            control_enabled(&mut tx, auth, ControlKind::Family, family).await?;
        }
        Ok(())
    }
}
pub(crate) async fn execution_gate(
    c: &mut AnyConnection,
    auth: &Authority,
    job: &Job,
) -> Result<()> {
    control_enabled(
        c,
        auth,
        ControlKind::Profile,
        &job.spec.brief.profile.id.to_string(),
    )
    .await?;
    if sqlx::query("SELECT id FROM decision_requests WHERE tenant_id=$1 AND job_id=$2 AND (answer IS NULL OR answer<>'approve') LIMIT 1").bind(auth.tenant_id.to_string()).bind(job.id.to_string()).fetch_optional(c).await?.is_some() { return Err(Error::Invalid("Job awaits an explicit owner decision; silence is not approval".into())); }
    Ok(())
}
async fn control_enabled(
    c: &mut AnyConnection,
    auth: &Authority,
    kind: ControlKind,
    target: &str,
) -> Result<()> {
    let row = sqlx::query(
        "SELECT enabled FROM runtime_controls WHERE tenant_id=$1 AND kind=$2 AND target=$3",
    )
    .bind(auth.tenant_id.to_string())
    .bind(tag(&kind)?)
    .bind(target)
    .fetch_optional(c)
    .await?;
    if row.is_some_and(|r| r.get::<i64, _>("enabled") == 0) {
        return Err(Error::Invalid(format!(
            "{} {target} is disabled",
            tag(&kind)?
        )));
    }
    Ok(())
}
async fn exploration(c: &mut AnyConnection, auth: &Authority, key: Uuid) -> Result<Exploration> {
    read_scope(c, auth, key).await?;
    let row = sqlx::query("SELECT data FROM explorations WHERE tenant_id=$1 AND id=$2")
        .bind(auth.tenant_id.to_string())
        .bind(key.to_string())
        .fetch_optional(c)
        .await?
        .ok_or(Error::NotFound)?;
    let result: Exploration = serde_json::from_str(row.try_get("data")?)?;
    if result.expires_at <= chrono::Utc::now() {
        return Err(Error::Unavailable);
    }
    Ok(result)
}
