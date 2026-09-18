use crate::{
    Error, Result, Store,
    access::{predicate, read_scope, same_scope, write_scope},
    database::{changed, id, number, tag, timestamp},
    policies::current_policy,
    temporal,
};
use memory_domain::{
    contracts::{Authority, MemoryRef},
    records::{
        CommitPosition, MemoryChange, MemoryContent, MemoryQuery, MemoryVersion, ProcedureForm,
        Qualification, RecordDraft,
    },
};
use sqlx::{AnyConnection, Row, any::AnyRow};
use uuid::Uuid;

impl Store {
    pub async fn create_memory(
        &self,
        authority: &Authority,
        record: RecordDraft,
    ) -> Result<MemoryVersion> {
        let mut versions = self.create_memories(authority, vec![record]).await?;
        Ok(versions.remove(0))
    }

    pub async fn create_memories(
        &self,
        authority: &Authority,
        records: Vec<RecordDraft>,
    ) -> Result<Vec<MemoryVersion>> {
        self.apply_memories(
            authority,
            records.into_iter().map(MemoryChange::Create).collect(),
        )
        .await
    }

    pub async fn revise_memory(
        &self,
        authority: &Authority,
        expected: &MemoryRef,
        record: RecordDraft,
    ) -> Result<MemoryVersion> {
        let mut versions = self
            .apply_memories(
                authority,
                vec![MemoryChange::Revise {
                    expected: expected.clone(),
                    record,
                }],
            )
            .await?;
        Ok(versions.remove(0))
    }

    /// Corrections and new assertions share one transaction and recorded position.
    /// Failure rolls back every record, policy decision and derivation edge.
    pub async fn apply_memories(
        &self,
        authority: &Authority,
        changes: Vec<MemoryChange>,
    ) -> Result<Vec<MemoryVersion>> {
        let (mut tx, position) = self.begin_write().await?;
        let results = apply_changes(&mut tx, authority, changes, position).await?;
        tx.commit().await?;
        Ok(results)
    }

    pub async fn memory(
        &self,
        authority: &Authority,
        reference: &MemoryRef,
    ) -> Result<MemoryVersion> {
        memory_version(&mut *self.pool.acquire().await?, authority, reference).await
    }

    pub async fn memories(
        &self,
        authority: &Authority,
        query: &MemoryQuery,
    ) -> Result<Vec<MemoryVersion>> {
        let (predicate, mut values) = predicate(authority);
        let mut sql = format!(
            "SELECT v.*, m.created_by FROM memory_versions v JOIN memory_records m ON m.tenant_id=v.tenant_id AND m.id=v.id JOIN resource_scopes s ON s.tenant_id=m.tenant_id AND s.id=m.id WHERE {predicate}"
        );
        temporal::filter(&mut sql, &mut values, query);
        if !query.include_inactive {
            sql.push_str(" AND v.availability='routine'");
        }
        if let Some(entity) = query.entity_id {
            values.push(entity.to_string());
            sql.push_str(&format!(" AND EXISTS (SELECT 1 FROM scope_entities e WHERE e.tenant_id=s.tenant_id AND e.scope_id=s.id AND e.entity_id=${})", values.len()));
        }
        if let Some(after) = query.after_id {
            values.push(after.to_string());
            sql.push_str(&format!(" AND v.id > ${}", values.len()));
        }
        sql.push_str(&format!(
            " ORDER BY v.id LIMIT {}",
            query.limit.clamp(1, 1000)
        ));
        let mut statement = sqlx::query(&sql);
        for value in values {
            statement = statement.bind(value);
        }
        let rows = statement.fetch_all(&self.pool).await?;
        rows.iter().map(decode_memory).collect()
    }
}

pub(crate) async fn validate_record(
    connection: &mut AnyConnection,
    authority: &Authority,
    record: &RecordDraft,
    position: CommitPosition,
) -> Result<()> {
    record.validate().map_err(Error::Invalid)?;
    if !authority.scope.permits(&record.scope) {
        return Err(Error::Forbidden);
    }
    let policy = current_policy(connection, authority, &record.decision.policy).await?;
    if !policy.scope.permits(&record.scope) {
        return Err(Error::Forbidden);
    }
    if record
        .decision
        .expires_at
        .is_some_and(|expires| expires <= position.recorded_at)
    {
        return Err(Error::Invalid("Policy decision has expired".into()));
    }
    for entity in &record.scope.entity_ids {
        crate::entities::entity(connection, authority, *entity).await?;
    }
    if let MemoryContent::Knowledge {
        subject: Some(subject),
        ..
    } = &record.content
    {
        crate::entities::entity(connection, authority, *subject).await?;
    }
    for source in &record.scope.source_versions {
        crate::sources::source_version(connection, authority, source).await?;
    }
    for locator_id in &record.source_locators {
        crate::sources::locator(connection, authority, *locator_id).await?;
    }
    for input in &record.derived_from {
        memory_version(connection, authority, input).await?;
    }
    if let Qualification::Evaluated {
        evidence,
        conditions,
        ..
    } = &record.qualification
    {
        if evidence.is_empty() || conditions.is_empty() {
            return Err(Error::Invalid(
                "Qualification needs evidence and conditions".into(),
            ));
        }
        for input in evidence {
            memory_version(connection, authority, input).await?;
        }
    }
    if let MemoryContent::Procedure {
        method,
        counterexamples,
        ..
    } = &record.content
    {
        for input in counterexamples {
            memory_version(connection, authority, input).await?;
        }
        if let ProcedureForm::Executable { artifact_id, .. } = method {
            crate::artifacts::ready_artifact(connection, authority, *artifact_id).await?;
        }
    }
    Ok(())
}

async fn insert_version(
    connection: &mut AnyConnection,
    authority: &Authority,
    id: Uuid,
    revision: u32,
    record: RecordDraft,
    recorded: CommitPosition,
) -> Result<MemoryVersion> {
    let tenant = authority.tenant_id.to_string();
    let decision_id = Uuid::now_v7();
    let version_id = Uuid::now_v7();
    sqlx::query("INSERT INTO policy_decisions (tenant_id,id,policy_id,policy_revision,actor_id,sequence,data) VALUES ($1,$2,$3,$4,$5,$6,$7)")
        .bind(&tenant).bind(decision_id.to_string()).bind(record.decision.policy.id.to_string()).bind(i64::from(record.decision.policy.revision.get())).bind(authority.actor_id.to_string()).bind(i64::from(recorded.sequence)).bind(serde_json::to_string(&record.decision)?).execute(&mut *connection).await?;
    let (known, from, to) = temporal::interval(&record.valid_time);
    sqlx::query("INSERT INTO memory_versions (tenant_id,id,revision,version_id,recorded_from,recorded_at,valid_known,valid_from,valid_to,availability,decision_id,data) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)")
        .bind(&tenant).bind(id.to_string()).bind(i64::from(revision)).bind(version_id.to_string()).bind(i64::from(recorded.sequence)).bind(recorded.recorded_at.timestamp_millis()).bind(known).bind(from).bind(to).bind(tag(&record.availability)?).bind(decision_id.to_string()).bind(serde_json::to_string(&record)?).execute(&mut *connection).await?;
    crate::activation::index_record(
        connection,
        &tenant,
        &version_id.to_string(),
        i64::from(recorded.sequence),
        &record,
    )
    .await?;
    let reference = MemoryRef {
        memory_id: id,
        revision: revision.try_into().unwrap(),
        label: record.label.clone(),
    };
    for locator in &record.source_locators {
        sqlx::query("INSERT INTO memory_sources (tenant_id,memory_id,revision,locator_id) VALUES ($1,$2,$3,$4) ON CONFLICT DO NOTHING").bind(&tenant).bind(id.to_string()).bind(i64::from(revision)).bind(locator.to_string()).execute(&mut *connection).await?;
    }
    for input in &record.derived_from {
        crate::relations::insert_derivation(
            connection,
            authority,
            &reference,
            input,
            &record.scope,
            recorded,
        )
        .await?;
    }
    let created_by =
        sqlx::query("SELECT created_by FROM memory_records WHERE tenant_id=$1 AND id=$2")
            .bind(&tenant)
            .bind(id.to_string())
            .fetch_one(&mut *connection)
            .await?
            .try_get::<String, _>("created_by")?;
    Ok(MemoryVersion {
        reference,
        version_id,
        created_by: crate::database::id(&created_by)?,
        recorded,
        recorded_until: None,
        decision_id,
        record,
    })
}

pub(crate) async fn memory_version(
    connection: &mut AnyConnection,
    authority: &Authority,
    reference: &MemoryRef,
) -> Result<MemoryVersion> {
    read_scope(connection, authority, reference.memory_id).await?;
    let row = sqlx::query("SELECT v.*, m.created_by FROM memory_versions v JOIN memory_records m ON m.tenant_id=v.tenant_id AND m.id=v.id WHERE v.tenant_id=$1 AND v.id=$2 AND v.revision=$3")
        .bind(authority.tenant_id.to_string()).bind(reference.memory_id.to_string()).bind(i64::from(reference.revision.get())).fetch_optional(connection).await?.ok_or(Error::NotFound)?;
    decode_memory(&row)
}

pub(crate) fn decode_memory(row: &AnyRow) -> Result<MemoryVersion> {
    let record: RecordDraft = serde_json::from_str(row.try_get("data")?)?;
    Ok(MemoryVersion {
        reference: MemoryRef {
            memory_id: id(row.try_get("id")?)?,
            revision: number(row.try_get("revision")?)?
                .try_into()
                .map_err(|_| Error::Invalid("Zero revision".into()))?,
            label: record.label.clone(),
        },
        version_id: id(row.try_get("version_id")?)?,
        created_by: id(row.try_get("created_by")?)?,
        recorded: CommitPosition {
            sequence: number(row.try_get("recorded_from")?)?,
            recorded_at: timestamp(row.try_get("recorded_at")?)?,
        },
        recorded_until: row
            .try_get::<Option<i64>, _>("recorded_to")?
            .map(number)
            .transpose()?,
        decision_id: id(row.try_get("decision_id")?)?,
        record,
    })
}

pub(crate) async fn apply_changes(
    connection: &mut AnyConnection,
    authority: &Authority,
    changes: Vec<MemoryChange>,
    position: CommitPosition,
) -> Result<Vec<MemoryVersion>> {
    if changes.is_empty() {
        return Err(Error::Invalid("Empty memory batch".into()));
    }
    let mut results = Vec::new();
    let mut revised = std::collections::HashSet::new();
    for change in changes {
        let (id, revision, record) = match change {
            MemoryChange::Create(record) => {
                validate_record(connection, authority, &record, position).await?;
                let id = Uuid::now_v7();
                write_scope(connection, authority, id, &record.scope).await?;
                sqlx::query("INSERT INTO memory_records (tenant_id,id,family,revision,created_by) VALUES ($1,$2,$3,1,$4)")
                    .bind(authority.tenant_id.to_string()).bind(id.to_string()).bind(record.content.family()).bind(authority.actor_id.to_string()).execute(&mut *connection).await?;
                (id, 1, record)
            }
            MemoryChange::Revise { expected, record } => {
                if !revised.insert(expected.memory_id) {
                    return Err(Error::Invalid("A batch can revise each memory once".into()));
                }
                let previous = memory_version(connection, authority, &expected).await?;
                if !same_scope(&previous.record.scope, &record.scope)
                    || previous.record.content.family() != record.content.family()
                {
                    return Err(Error::Invalid(
                        "Scope or family changes require a new contribution".into(),
                    ));
                }
                validate_record(connection, authority, &record, position).await?;
                let revision = expected
                    .revision
                    .get()
                    .checked_add(1)
                    .ok_or_else(|| Error::Invalid("Revision overflow".into()))?;
                changed(sqlx::query("UPDATE memory_records SET revision=$1 WHERE tenant_id=$2 AND id=$3 AND revision=$4")
                    .bind(i64::from(revision)).bind(authority.tenant_id.to_string()).bind(expected.memory_id.to_string()).bind(i64::from(expected.revision.get())).execute(&mut *connection).await?.rows_affected())?;
                sqlx::query("UPDATE memory_versions SET recorded_to=$1 WHERE tenant_id=$2 AND id=$3 AND recorded_to IS NULL")
                    .bind(i64::from(position.sequence)).bind(authority.tenant_id.to_string()).bind(expected.memory_id.to_string()).execute(&mut *connection).await?;
                (expected.memory_id, revision, record)
            }
        };
        let version = insert_version(connection, authority, id, revision, record, position).await?;
        crate::intentions::sync_definition(connection, authority, &version, position).await?;
        crate::coordinator::events::append_event(
            connection,
            authority,
            id,
            "memory_revised",
            position,
            position.recorded_at + chrono::Duration::days(30),
        )
        .await?;
        results.push(version);
    }
    Ok(results)
}
