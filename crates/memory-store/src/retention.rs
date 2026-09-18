use crate::{
    Error, Result, Store,
    access::{read_scope, require_write_scope},
    database::id,
};
use memory_domain::{contracts::Authority, retention::*};
use serde_json::Value;
use sqlx::{AnyConnection, Row};
use std::collections::BTreeSet;
use uuid::Uuid;

const MAX_RESOURCES: usize = 10_000;

pub(crate) async fn check_resource(
    db: &mut AnyConnection,
    auth: &Authority,
    resource: Uuid,
) -> Result<()> {
    if sqlx::query("SELECT epoch FROM revoked_resources WHERE tenant_id=$1 AND resource_id=$2")
        .bind(auth.tenant_id.to_string())
        .bind(resource.to_string())
        .fetch_optional(db)
        .await?
        .is_some()
    {
        return Err(Error::Unavailable);
    }
    Ok(())
}
fn references(value: &Value, ids: &BTreeSet<String>) -> bool {
    match value {
        Value::String(s) => ids.contains(s),
        Value::Array(a) => a.iter().any(|v| references(v, ids)),
        Value::Object(o) => o.values().any(|v| references(v, ids)),
        _ => false,
    }
}
fn collect_ids(value: &Value, ids: &mut BTreeSet<Uuid>) {
    match value {
        Value::String(s) => {
            if let Ok(id) = s.parse() {
                ids.insert(id);
            }
        }
        Value::Array(a) => a.iter().for_each(|v| collect_ids(v, ids)),
        Value::Object(o) => o.values().for_each(|v| collect_ids(v, ids)),
        _ => (),
    }
}
async fn rows(db: &mut AnyConnection, tenant: &str, table: &str) -> Result<Vec<sqlx::any::AnyRow>> {
    let rows = sqlx::query(&format!(
        "SELECT * FROM {table} WHERE tenant_id=$1 LIMIT 10001"
    ))
    .bind(tenant)
    .fetch_all(db)
    .await?;
    if rows.len() > MAX_RESOURCES {
        return Err(Error::LimitExceeded);
    }
    Ok(rows)
}
async fn remove(
    db: &mut AnyConnection,
    tenant: &str,
    table: &str,
    column: &str,
    value: &str,
) -> Result<()> {
    sqlx::query(&format!(
        "DELETE FROM {table} WHERE tenant_id=$1 AND {column}=$2"
    ))
    .bind(tenant)
    .bind(value)
    .execute(db)
    .await?;
    Ok(())
}

impl Store {
    pub async fn deletions(
        &self,
        auth: &Authority,
        after: Option<Uuid>,
        limit: u16,
    ) -> Result<Vec<DeletionReport>> {
        if limit == 0 || limit > 100 {
            return Err(Error::LimitExceeded);
        }
        let mut connection = self.pool.acquire().await?;
        let mut reports = rows(
            &mut connection,
            &auth.tenant_id.to_string(),
            "deletion_jobs",
        )
        .await?
        .into_iter()
        .map(|row| {
            Ok(serde_json::from_str::<DeletionReport>(
                row.try_get("data")?,
            )?)
        })
        .collect::<Result<Vec<_>>>()?;
        reports.retain(|r| auth.scope.permits(&r.scope) && after.is_none_or(|id| r.id > id));
        reports.sort_by_key(|r| r.id);
        reports.truncate(usize::from(limit));
        Ok(reports)
    }

    /// Called before returning worker data. The write lock serializes exposure with revocation.
    pub async fn record_exposure(&self, auth: &Authority, job: Uuid, value: &Value) -> Result<()> {
        let (mut tx, _) = self.begin_write().await?;
        read_scope(&mut tx, auth, job).await?;
        let mut ids = BTreeSet::new();
        collect_ids(value, &mut ids);
        if ids.len() > 2048 {
            return Err(Error::LimitExceeded);
        }
        for resource in ids {
            check_resource(&mut tx, auth, resource).await?;
            let mut scopes =
                sqlx::query("SELECT id FROM resource_scopes WHERE tenant_id=$1 AND id=$2")
                    .bind(auth.tenant_id.to_string())
                    .bind(resource.to_string())
                    .fetch_all(&mut *tx)
                    .await?;
            scopes.extend(sqlx::query("SELECT scope_id AS id FROM source_versions WHERE tenant_id=$1 AND source_id=$2")
                .bind(auth.tenant_id.to_string()).bind(resource.to_string()).fetch_all(&mut *tx).await?);
            let mut readable = false;
            for scope in scopes {
                match read_scope(&mut tx, auth, id(scope.try_get("id")?)?).await {
                    Ok(_) => readable = true,
                    Err(Error::NotFound) => (),
                    Err(error) => return Err(error),
                }
            }
            // Failed or empty out-of-scope reads must not poison an unrelated resource's
            // deletion plan. Record/source identities cover their version and locator copies.
            if !readable {
                continue;
            }
            sqlx::query("INSERT INTO resource_exposures(tenant_id,job_id,resource_id) VALUES($1,$2,$3) ON CONFLICT DO NOTHING")
                .bind(auth.tenant_id.to_string()).bind(job.to_string()).bind(resource.to_string()).execute(&mut *tx).await?;
        }
        let count: i64 = sqlx::query(
            "SELECT COUNT(*) AS n FROM resource_exposures WHERE tenant_id=$1 AND job_id=$2",
        )
        .bind(auth.tenant_id.to_string())
        .bind(job.to_string())
        .fetch_one(&mut *tx)
        .await?
        .try_get("n")?;
        if count > MAX_RESOURCES as i64 {
            return Err(Error::LimitExceeded);
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn deletion_report(
        &self,
        auth: &Authority,
        deletion: Uuid,
    ) -> Result<DeletionReport> {
        let row = sqlx::query("SELECT data FROM deletion_jobs WHERE tenant_id=$1 AND id=$2")
            .bind(auth.tenant_id.to_string())
            .bind(deletion.to_string())
            .fetch_optional(&self.pool)
            .await?
            .ok_or(Error::NotFound)?;
        let mut report: DeletionReport = serde_json::from_str(row.try_get("data")?)?;
        if !auth.scope.permits(&report.scope) {
            return Err(Error::Forbidden);
        }
        for effect in &mut report.effects {
            let row = sqlx::query("SELECT state FROM effect_requests WHERE tenant_id=$1 AND id=$2")
                .bind(auth.tenant_id.to_string())
                .bind(effect.id.to_string())
                .fetch_optional(&self.pool)
                .await?;
            if let Some(row) = row {
                effect.state = crate::database::from_tag(row.try_get("state")?)?;
            }
        }
        Ok(report)
    }

    /// Plans the bounded affected containers and commits denial before any slow physical cleanup.
    pub async fn begin_deletion(
        &self,
        auth: &Authority,
        request: DeletionRequest,
    ) -> Result<DeletionReport> {
        if request.resources.is_empty() || request.resources.len() > 100 {
            return Err(Error::LimitExceeded);
        }
        let (mut tx, position) = self.begin_write().await?;
        let tenant = auth.tenant_id.to_string();
        if let Some(row) =
            sqlx::query("SELECT data FROM deletion_jobs WHERE tenant_id=$1 AND id=$2")
                .bind(&tenant)
                .bind(request.id.to_string())
                .fetch_optional(&mut *tx)
                .await?
        {
            let prior: DeletionReport = serde_json::from_str(row.try_get("data")?)?;
            if !auth.scope.permits(&prior.scope) || request.resources != prior.requested_resources {
                return Err(Error::Conflict);
            }
            return Ok(prior);
        }
        let mut affected: BTreeSet<String> =
            request.resources.iter().map(ToString::to_string).collect();
        let mut source_aliases = BTreeSet::new();
        for resource in &request.resources {
            let versions = sqlx::query(
                "SELECT scope_id FROM source_versions WHERE tenant_id=$1 AND source_id=$2",
            )
            .bind(&tenant)
            .bind(resource.to_string())
            .fetch_all(&mut *tx)
            .await?;
            if !versions.is_empty() {
                affected.remove(&resource.to_string());
                source_aliases.insert(resource.to_string());
                for version in versions {
                    let scope: String = version.try_get("scope_id")?;
                    require_write_scope(&mut tx, auth, id(&scope)?).await?;
                    affected.insert(scope);
                }
                continue;
            }
            require_write_scope(&mut tx, auth, *resource).await?;
            let mut supported = false;
            for (table, column) in [
                ("explorations", "id"),
                ("decision_requests", "id"),
                ("task_contexts", "id"),
                ("memory_records", "id"),
                ("artifact_records", "id"),
                ("jobs", "id"),
                ("source_versions", "scope_id"),
            ] {
                supported |= sqlx::query(&format!(
                    "SELECT {column} FROM {table} WHERE tenant_id=$1 AND {column}=$2"
                ))
                .bind(&tenant)
                .bind(resource.to_string())
                .fetch_optional(&mut *tx)
                .await?
                .is_some();
            }
            if !supported {
                return Err(Error::Invalid(
                    "Deletion requires a memory, artifact, job or source scope identity".into(),
                ));
            }
        }
        let mut interaction_rows = vec![];
        for table in ["explorations", "decision_requests", "task_contexts"] {
            interaction_rows.extend(rows(&mut tx, &tenant, table).await?);
        }
        let memories = rows(&mut tx, &tenant, "memory_versions").await?;
        let sources = rows(&mut tx, &tenant, "source_versions").await?;
        let locators = rows(&mut tx, &tenant, "source_locators").await?;
        let artifacts = rows(&mut tx, &tenant, "artifact_records").await?;
        let jobs = rows(&mut tx, &tenant, "jobs").await?;
        let exposures = rows(&mut tx, &tenant, "resource_exposures").await?;
        let outputs = rows(&mut tx, &tenant, "job_artifacts").await?;
        let relations = rows(&mut tx, &tenant, "relation_records").await?;
        let mut aliases = affected.clone();
        aliases.extend(source_aliases);
        loop {
            let before = (affected.len(), aliases.len());
            for row in &sources {
                let scope: String = row.try_get("scope_id")?;
                let snapshot: Option<String> = row.try_get("snapshot_artifact")?;
                if aliases.contains(row.try_get::<&str, _>("source_id")?)
                    || affected.contains(&scope)
                    || snapshot.as_ref().is_some_and(|a| affected.contains(a))
                {
                    affected.insert(scope);
                    aliases.insert(row.try_get("source_id")?);
                    if let Some(snapshot) = snapshot {
                        affected.insert(snapshot);
                    }
                }
            }
            for row in &locators {
                if aliases.contains(row.try_get::<&str, _>("source_id")?) {
                    aliases.insert(row.try_get("id")?);
                }
            }
            for row in &memories {
                let record: Value = serde_json::from_str(row.try_get("data")?)?;
                let memory: String = row.try_get("id")?;
                // Supports edges are deliberately not a content derivation. Provenance in the
                // record is governed content, including historical versions of the same record.
                if references(&record["derived_from"], &aliases)
                    || references(&record["source_locators"], &aliases)
                    || affected.contains(&memory)
                {
                    affected.insert(memory);
                    aliases.insert(row.try_get("version_id")?);
                }
            }
            for row in &interaction_rows {
                let data: Value = serde_json::from_str(row.try_get("data")?)?;
                if references(&data, &aliases)
                    || row
                        .try_get::<String, _>("job_id")
                        .is_ok_and(|j| affected.contains(&j))
                {
                    affected.insert(row.try_get("id")?);
                }
            }
            for row in &artifacts {
                let spec: Value = serde_json::from_str(row.try_get("spec")?)?;
                if references(&spec["dependencies"], &aliases) {
                    affected.insert(row.try_get("id")?);
                }
            }
            for row in &relations {
                let from: String = row.try_get("from_id")?;
                let to: String = row.try_get("to_id")?;
                let kind: String = row.try_get("kind")?;
                if ["derived_from", "depends_on"].contains(&kind.as_str()) && affected.contains(&to)
                {
                    affected.insert(from.clone());
                }
                if affected.contains(&from) || affected.contains(&to) {
                    affected.insert(row.try_get("id")?);
                }
            }
            for row in &exposures {
                if aliases.contains(row.try_get::<&str, _>("resource_id")?) {
                    affected.insert(row.try_get("job_id")?);
                }
            }
            for row in &jobs {
                for column in ["spec", "result"] {
                    if let Some(data) = row.try_get::<Option<String>, _>(column)? {
                        if references(&serde_json::from_str(&data)?, &aliases) {
                            affected.insert(row.try_get("id")?);
                        }
                    }
                }
            }
            let sessions: BTreeSet<String> = jobs
                .iter()
                .filter(|r| {
                    r.try_get::<String, _>("id")
                        .is_ok_and(|id| affected.contains(&id))
                })
                .map(|r| r.try_get("session_id"))
                .collect::<std::result::Result<_, _>>()?;
            for row in &jobs {
                if sessions.contains(row.try_get::<&str, _>("session_id")?) {
                    affected.insert(row.try_get("id")?);
                }
            }
            for row in &outputs {
                if affected.contains(row.try_get::<&str, _>("job_id")?) {
                    affected.insert(row.try_get("artifact_id")?);
                }
            }
            aliases.extend(affected.iter().cloned());
            if affected.len() > MAX_RESOURCES || aliases.len() > MAX_RESOURCES {
                return Err(Error::LimitExceeded);
            }
            if before == (affected.len(), aliases.len()) {
                break;
            }
        }
        let resources = affected.iter().map(|s| id(s)).collect::<Result<Vec<_>>>()?;
        for resource in &resources {
            // A previous barrier is safe to include in a later overlapping deletion.
            match check_resource(&mut tx, auth, *resource).await {
                Ok(()) => require_write_scope(&mut tx, auth, *resource).await?,
                Err(Error::Unavailable) => (),
                Err(error) => return Err(error),
            }
        }
        let mut report = DeletionReport {
            id: request.id,
            tenant_id: auth.tenant_id,
            epoch: position.sequence,
            requested_resources: request.resources.clone(),
            revoked_ids: aliases
                .iter()
                .map(|value| id(value))
                .collect::<Result<_>>()?,
            scope: auth.scope.clone(),
            resources,
            support_review: relations
                .iter()
                .filter_map(|r| {
                    let from: String = r.try_get("from_id").ok()?;
                    let to: String = r.try_get("to_id").ok()?;
                    (r.try_get::<String, _>("kind").ok()? == "supports"
                        && affected.contains(&from)
                        && !affected.contains(&to))
                    .then(|| id(&to).ok())
                    .flatten()
                })
                .collect(),
            sessions: vec![],
            effects: vec![],
            access_denied: true,
            live_payloads_removed: false,
            obligations: vec![],
        };
        for row in &jobs {
            let job: String = row.try_get("id")?;
            if !affected.contains(&job) {
                continue;
            }
            let session: String = row.try_get("session_id")?;
            if !report.sessions.contains(&session) {
                report.sessions.push(session.clone());
            }
            for (kind, container) in [
                (RetentionBoundary::Session, session.clone()),
                (RetentionBoundary::Provider, job.clone()),
                (RetentionBoundary::Sandbox, job.clone()),
            ] {
                if !report
                    .obligations
                    .iter()
                    .any(|o| o.kind == kind && o.container == container)
                {
                    report.obligations.push(PurgeObligation {
                        kind,
                        container,
                        acknowledged: false,
                    });
                }
            }
            sqlx::query("INSERT INTO deleted_sessions(tenant_id,session_id) VALUES($1,$2) ON CONFLICT DO NOTHING").bind(&tenant).bind(&session).execute(&mut *tx).await?;
            sqlx::query("UPDATE session_leases SET epoch=epoch+1,expires_at=0 WHERE tenant_id=$1 AND session_id=$2").bind(&tenant).bind(&session).execute(&mut *tx).await?;
            sqlx::query(
                "UPDATE jobs SET cancel_requested=1,state='cancelled' WHERE tenant_id=$1 AND id=$2",
            )
            .bind(&tenant)
            .bind(&job)
            .execute(&mut *tx)
            .await?;
        }
        for effect in rows(&mut tx, &tenant, "effect_requests").await? {
            if affected.contains(effect.try_get::<&str, _>("job_id")?) {
                let effect_id: String = effect.try_get("id")?;
                let mut state: memory_domain::coordination::EffectState =
                    crate::database::from_tag(effect.try_get("state")?)?;
                if state == memory_domain::coordination::EffectState::InProgress {
                    state = memory_domain::coordination::EffectState::OutcomeUnknown;
                    sqlx::query("UPDATE effect_requests SET state='outcome_unknown' WHERE tenant_id=$1 AND id=$2")
                        .bind(&tenant).bind(&effect_id).execute(&mut *tx).await?;
                }
                report.effects.push(DeletedEffect {
                    id: id(&effect_id)?,
                    state,
                    evidence_artifact: None,
                });
            }
        }
        for artifact in &artifacts {
            let resource: String = artifact.try_get("id")?;
            if affected.contains(&resource)
                && artifact.try_get::<String, _>("state")? == "uploading"
            {
                report.obligations.push(PurgeObligation {
                    kind: RetentionBoundary::Upload,
                    container: resource,
                    acknowledged: false,
                });
            }
        }
        for kind in [
            RetentionBoundary::Backup,
            RetentionBoundary::DatabaseStorage,
        ] {
            report.obligations.push(PurgeObligation {
                kind,
                container: tenant.clone(),
                acknowledged: false,
            });
        }
        for resource in &aliases {
            sqlx::query("INSERT INTO revoked_resources(tenant_id,resource_id,epoch) VALUES($1,$2,$3) ON CONFLICT DO NOTHING").bind(&tenant).bind(resource).bind(i64::from(position.sequence)).execute(&mut *tx).await?;
        }
        sqlx::query("INSERT INTO deletion_jobs(tenant_id,id,data) VALUES($1,$2,$3)")
            .bind(&tenant)
            .bind(request.id.to_string())
            .bind(serde_json::to_string(&report)?)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(report)
    }

    pub async fn acknowledge_purge(
        &self,
        auth: &Authority,
        deletion: Uuid,
        kind: RetentionBoundary,
        container: &str,
    ) -> Result<DeletionReport> {
        self.deletion_report(auth, deletion).await?;
        let (mut tx, _) = self.begin_write().await?;
        // Reload under the write lock so parallel acknowledgements cannot overwrite each other.
        let row = sqlx::query("SELECT data FROM deletion_jobs WHERE tenant_id=$1 AND id=$2")
            .bind(auth.tenant_id.to_string())
            .bind(deletion.to_string())
            .fetch_one(&mut *tx)
            .await?;
        let mut report: DeletionReport = serde_json::from_str(row.try_get("data")?)?;
        report
            .obligations
            .iter_mut()
            .find(|o| o.kind == kind && o.container == container)
            .ok_or(Error::NotFound)?
            .acknowledged = true;
        save(&mut tx, auth, &report).await?;
        tx.commit().await?;
        Ok(report)
    }

    /// Restore administrators must apply this separately retained registry before opening access.
    pub async fn restore_deletion_registry(
        &self,
        auth: &Authority,
        reports: Vec<DeletionReport>,
    ) -> Result<()> {
        let (mut tx, _) = self.begin_write().await?;
        for mut report in reports {
            if report.tenant_id != auth.tenant_id || !auth.scope.permits(&report.scope) {
                return Err(Error::Forbidden);
            }
            for resource in &report.revoked_ids {
                sqlx::query("INSERT INTO revoked_resources(tenant_id,resource_id,epoch) VALUES($1,$2,$3) ON CONFLICT DO NOTHING").bind(auth.tenant_id.to_string()).bind(resource.to_string()).bind(i64::from(report.epoch)).execute(&mut *tx).await?;
            }
            for session in &report.sessions {
                sqlx::query("INSERT INTO deleted_sessions(tenant_id,session_id) VALUES($1,$2) ON CONFLICT DO NOTHING").bind(auth.tenant_id.to_string()).bind(session).execute(&mut *tx).await?;
                sqlx::query("UPDATE session_leases SET epoch=epoch+1,expires_at=0 WHERE tenant_id=$1 AND session_id=$2").bind(auth.tenant_id.to_string()).bind(session).execute(&mut *tx).await?;
            }
            report.live_payloads_removed = false;
            report.access_denied = true;
            for obligation in &mut report.obligations {
                obligation.acknowledged = false;
            }
            sqlx::query("INSERT INTO deletion_jobs(tenant_id,id,data) VALUES($1,$2,$3) ON CONFLICT(tenant_id,id) DO UPDATE SET data=excluded.data").bind(auth.tenant_id.to_string()).bind(report.id.to_string()).bind(serde_json::to_string(&report)?).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }
}
async fn save(db: &mut AnyConnection, auth: &Authority, report: &DeletionReport) -> Result<()> {
    sqlx::query("UPDATE deletion_jobs SET data=$1 WHERE tenant_id=$2 AND id=$3")
        .bind(serde_json::to_string(report)?)
        .bind(auth.tenant_id.to_string())
        .bind(report.id.to_string())
        .execute(db)
        .await?;
    Ok(())
}

impl Store {
    /// Removes content-bearing SQL rows. Minimal identities and effect outcomes remain fenced.
    pub(crate) async fn purge_deletion_rows(
        &self,
        auth: &Authority,
        deletion: Uuid,
    ) -> Result<DeletionReport> {
        self.deletion_report(auth, deletion).await?;
        let (mut tx, _) = self.begin_write().await?;
        let tenant = auth.tenant_id.to_string();
        let row = sqlx::query("SELECT data FROM deletion_jobs WHERE tenant_id=$1 AND id=$2")
            .bind(&tenant)
            .bind(deletion.to_string())
            .fetch_one(&mut *tx)
            .await?;
        let mut report: DeletionReport = serde_json::from_str(row.try_get("data")?)?;
        let mut affected: BTreeSet<String> =
            report.resources.iter().map(ToString::to_string).collect();
        let mut decisions = BTreeSet::new();
        for row in rows(&mut tx, &tenant, "memory_versions").await? {
            if affected.contains(row.try_get::<&str, _>("id")?) {
                let version: String = row.try_get("version_id")?;
                decisions.insert(row.try_get::<String, _>("decision_id")?);
                affected.insert(version.clone());
                for table in ["memory_search", "search_entities"] {
                    sqlx::query(&format!("DELETE FROM {table} WHERE version_id=$1"))
                        .bind(&version)
                        .execute(&mut *tx)
                        .await?;
                }
                for table in ["memory_embeddings", "search_updates"] {
                    remove(&mut tx, &tenant, table, "version_id", &version).await?;
                }
            }
        }
        for row in rows(&mut tx, &tenant, "source_versions").await? {
            if affected.contains(row.try_get::<&str, _>("scope_id")?) {
                affected.insert(row.try_get("source_id")?);
            }
        }
        // These are bounded cached containers, not independent retained assertions. A cache
        // which copied affected content is removed as a whole, including its result text.
        for (table, key, columns) in [
            ("explorations", "id", vec!["data"]),
            ("decision_requests", "id", vec!["data"]),
            ("task_contexts", "id", vec!["data"]),
            ("semantic_assessments", "id", vec!["data"]),
            ("judgement_decisions", "id", vec!["data"]),
            ("task_local_checks", "id", vec!["data"]),
            ("formation_windows", "id", vec!["data"]),
            ("formation_results", "window_id", vec!["request", "result"]),
            ("activation_windows", "id", vec!["data"]),
            ("consolidation_windows", "id", vec!["data"]),
            ("consolidation_reviews", "id", vec!["data", "result"]),
            ("qualification_adoptions", "job_id", vec!["data"]),
            ("maintenance_reviews", "id", vec!["data", "result"]),
            ("intention_checks", "id", vec!["data"]),
            ("intention_occurrences", "id", vec!["data"]),
            ("memory_changes", "id", vec!["data"]),
            ("command_receipts", "request_id", vec!["request", "result"]),
            (
                "formation_events",
                "source_id",
                vec!["event", "records", "deferred"],
            ),
        ] {
            for row in rows(&mut tx, &tenant, table).await? {
                let linked = ["job_id", "resource_id", "definition_id", "source_id"]
                    .iter()
                    .any(|c| {
                        row.try_get::<String, _>(*c)
                            .is_ok_and(|v| affected.contains(&v))
                    });
                let mut tainted = linked || affected.contains(row.try_get::<&str, _>(key)?);
                for column in &columns {
                    if let Some(data) = row.try_get::<Option<String>, _>(*column)? {
                        tainted |= references(&serde_json::from_str(&data)?, &affected);
                    }
                }
                if tainted {
                    if table == "command_receipts" {
                        sqlx::query("UPDATE command_receipts SET request=NULL,result=NULL WHERE tenant_id=$1 AND request_id=$2").bind(&tenant).bind(row.try_get::<&str, _>(key)?).execute(&mut *tx).await?;
                    } else {
                        remove(&mut tx, &tenant, table, key, row.try_get(key)?).await?;
                    }
                }
            }
        }
        for resource in &report.resources {
            let resource = resource.to_string();
            for table in ["relation_versions", "relation_records"] {
                remove(&mut tx, &tenant, table, "id", &resource).await?;
            }
        }
        for resource in &report.resources {
            let resource = resource.to_string();
            for (table, column) in [
                ("memory_sources", "memory_id"),
                ("memory_versions", "id"),
                ("memory_records", "id"),
                ("artifact_dependencies", "artifact_id"),
                ("artifact_dependencies", "depends_on"),
                ("job_artifacts", "artifact_id"),
            ] {
                remove(&mut tx, &tenant, table, column, &resource).await?;
            }
            sqlx::query("UPDATE effect_requests SET logical_operation_id=id,kind='deleted',receipt=CASE WHEN request='{}' THEN receipt ELSE NULL END,request='{}' WHERE tenant_id=$1 AND job_id=$2").bind(&tenant).bind(&resource).execute(&mut *tx).await?;
            sqlx::query("UPDATE jobs SET spec='{}',result=NULL,wait_reason=NULL WHERE tenant_id=$1 AND id=$2").bind(&tenant).bind(&resource).execute(&mut *tx).await?;
            remove(&mut tx, &tenant, "resource_exposures", "job_id", &resource).await?;
        }
        for decision in decisions {
            remove(&mut tx, &tenant, "policy_decisions", "id", &decision).await?;
        }
        for row in rows(&mut tx, &tenant, "source_versions").await? {
            if affected.contains(row.try_get::<&str, _>("scope_id")?) {
                let source: &str = row.try_get("source_id")?;
                let revision: &str = row.try_get("revision")?;
                sqlx::query("DELETE FROM source_locators WHERE tenant_id=$1 AND source_id=$2 AND source_revision=$3").bind(&tenant).bind(source).bind(revision).execute(&mut *tx).await?;
                sqlx::query("DELETE FROM source_versions WHERE tenant_id=$1 AND source_id=$2 AND revision=$3").bind(&tenant).bind(source).bind(revision).execute(&mut *tx).await?;
                sqlx::query("DELETE FROM sources WHERE tenant_id=$1 AND id=$2 AND NOT EXISTS(SELECT 1 FROM source_versions WHERE tenant_id=$1 AND source_id=$2)").bind(&tenant).bind(source).execute(&mut *tx).await?;
                remove(&mut tx, &tenant, "formation_cursors", "source_id", source).await?;
            }
        }
        for resource in &report.resources {
            remove(
                &mut tx,
                &tenant,
                "artifact_records",
                "id",
                &resource.to_string(),
            )
            .await?;
        }
        report.live_payloads_removed = true;
        save(&mut tx, auth, &report).await?;
        tx.commit().await?;
        Ok(report)
    }
}

impl Store {
    /// The administrator supplies a fresh brief with surviving inputs. No old transcript,
    /// operation state or effect request is copied, and normal admission validates each input.
    pub async fn continue_deleted_work(
        &self,
        auth: &Authority,
        deletion: Uuid,
        old_job: Uuid,
        command: memory_domain::contracts::Command<memory_domain::coordination::SubmitJob>,
    ) -> Result<memory_domain::coordination::Job> {
        let report = self.deletion_report(auth, deletion).await?;
        if !report.resources.contains(&old_job)
            || !report.live_payloads_removed
            || command.payload.parent_id.is_some()
        {
            return Err(Error::Conflict);
        }
        // Require reconciliation before allowing a fresh assignment to make external effects.
        let unknown = report.effects.iter().any(|effect| {
            !matches!(
                effect.state,
                memory_domain::coordination::EffectState::Succeeded
                    | memory_domain::coordination::EffectState::Failed
            )
        });
        if unknown && !command.payload.brief.capabilities.tools.is_empty() {
            return Err(Error::Invalid(
                "Unresolved old effects require reconciliation before enabling continuation tools"
                    .into(),
            ));
        }
        self.submit_job(auth, command).await
    }
}

impl Store {
    /// Keep only a separately governed evidence reference. Not-performed is terminal here:
    /// erasure never puts an old invocation back into the execution queue.
    pub async fn reconcile_deleted_effect(
        &self,
        auth: &Authority,
        deletion: Uuid,
        effect_id: Uuid,
        resolution: memory_domain::coordination::EffectResolution,
        evidence_artifact: Uuid,
    ) -> Result<()> {
        let report = self.deletion_report(auth, deletion).await?;
        if !report.effects.iter().any(|effect| effect.id == effect_id) {
            return Err(Error::NotFound);
        }
        let (mut tx, _) = self.begin_write().await?;
        crate::artifacts::ready_artifact(&mut tx, auth, evidence_artifact).await?;
        let row = sqlx::query("SELECT data FROM deletion_jobs WHERE tenant_id=$1 AND id=$2")
            .bind(auth.tenant_id.to_string())
            .bind(deletion.to_string())
            .fetch_one(&mut *tx)
            .await?;
        let mut report: DeletionReport = serde_json::from_str(row.try_get("data")?)?;
        let summary = report
            .effects
            .iter_mut()
            .find(|e| e.id == effect_id)
            .ok_or(Error::NotFound)?;
        let state = match resolution {
            memory_domain::coordination::EffectResolution::Succeeded => "succeeded",
            memory_domain::coordination::EffectResolution::Failed
            | memory_domain::coordination::EffectResolution::NotPerformed => "failed",
        };
        let resolved = crate::database::from_tag(state.to_owned())?;
        if !matches!(
            summary.state,
            memory_domain::coordination::EffectState::Prepared
                | memory_domain::coordination::EffectState::OutcomeUnknown
        ) && summary.state != resolved
        {
            return Err(Error::Conflict);
        }
        if summary.evidence_artifact.is_some() {
            if summary.state == resolved && summary.evidence_artifact == Some(evidence_artifact) {
                return Ok(());
            }
            return Err(Error::Conflict);
        }
        sqlx::query("UPDATE effect_requests SET state=$1,receipt=$2 WHERE tenant_id=$3 AND id=$4")
            .bind(state)
            .bind(
                serde_json::json!({"resolution":resolution,"evidence_artifact":evidence_artifact})
                    .to_string(),
            )
            .bind(auth.tenant_id.to_string())
            .bind(effect_id.to_string())
            .execute(&mut *tx)
            .await?;
        summary.state = resolved;
        summary.evidence_artifact = Some(evidence_artifact);
        save(&mut tx, auth, &report).await?;
        tx.commit().await?;
        Ok(())
    }
}
