use crate::{
    Error, Result, Store,
    access::{predicate, read_scope, same_scope, write_scope},
    database::{changed, id, number, tag, timestamp},
    memories::memory_version,
    temporal,
};
use memory_domain::{
    contracts::{Authority, EvidentialStatus, MemoryRef, Scope},
    records::{
        Acceptance, CommitPosition, MemoryQuery, RelationDraft, RelationKind, RelationVersion,
        ValidTime,
    },
};
use sqlx::{AnyConnection, Row, any::AnyRow};
use std::collections::{HashSet, VecDeque};
use uuid::Uuid;

impl Store {
    pub async fn create_relation(
        &self,
        authority: &Authority,
        relation: RelationDraft,
    ) -> Result<RelationVersion> {
        let (mut tx, position) = self.begin_write().await?;
        let result = insert_relation(&mut tx, authority, relation, position).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn revise_relation(
        &self,
        authority: &Authority,
        id: Uuid,
        expected_revision: u32,
        relation: RelationDraft,
    ) -> Result<RelationVersion> {
        let (mut tx, position) = self.begin_write().await?;
        let result = revise_relation_in(
            &mut tx,
            authority,
            id,
            expected_revision,
            relation,
            position,
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn relations(
        &self,
        authority: &Authority,
        memory_id: Uuid,
        query: &MemoryQuery,
    ) -> Result<Vec<RelationVersion>> {
        let mut connection = self.pool.acquire().await?;
        read_scope(&mut connection, authority, memory_id).await?;
        let (predicate, mut values) = predicate(authority);
        values.push(memory_id.to_string());
        let endpoint = values.len();
        let mut sql = format!(
            "SELECT v.* FROM relation_versions v JOIN relation_records r ON r.tenant_id=v.tenant_id AND r.id=v.id JOIN resource_scopes s ON s.tenant_id=r.tenant_id AND s.id=r.id WHERE {predicate} AND (r.from_id=${endpoint} OR r.to_id=${endpoint})"
        );
        temporal::filter(&mut sql, &mut values, query);
        if let Some(after) = query.after_id {
            values.push(after.to_string());
            sql.push_str(&format!(" AND v.id > ${}", values.len()));
        }
        if let Some(entity) = query.entity_id {
            values.push(entity.to_string());
            sql.push_str(&format!(" AND EXISTS (SELECT 1 FROM scope_entities e WHERE e.tenant_id=s.tenant_id AND e.scope_id=s.id AND e.entity_id=${})", values.len()));
        }
        if !query.include_inactive {
            sql.push_str(" AND v.acceptance='accepted'");
        }
        sql.push_str(&format!(
            " ORDER BY v.id LIMIT {}",
            query.limit.clamp(1, 1000)
        ));
        let mut statement = sqlx::query(&sql);
        for value in values {
            statement = statement.bind(value);
        }
        let rows = statement.fetch_all(&mut *connection).await?;
        let mut visible = Vec::new();
        for row in rows {
            let relation = decode_relation(&row)?;
            // Endpoint access is checked independently; an edge never grants access.
            match (
                read_scope(&mut connection, authority, relation.relation.from.memory_id).await,
                read_scope(&mut connection, authority, relation.relation.to.memory_id).await,
            ) {
                (Ok(_), Ok(_)) => visible.push(relation),
                (Err(Error::NotFound), _) | (_, Err(Error::NotFound)) => (),
                (Err(error), _) | (_, Err(error)) => return Err(error),
            }
        }
        Ok(visible)
    }
}

fn endpoint(reference: &MemoryRef) -> (Uuid, u32) {
    (reference.memory_id, reference.revision.get())
}
fn canonical(mut relation: RelationDraft) -> RelationDraft {
    if relation.kind == RelationKind::ConflictsWith
        && endpoint(&relation.from) > endpoint(&relation.to)
    {
        std::mem::swap(&mut relation.from, &mut relation.to);
    }
    relation
}

async fn validate(
    connection: &mut AnyConnection,
    authority: &Authority,
    relation: &RelationDraft,
) -> Result<()> {
    if !authority.scope.permits(&relation.scope) {
        return Err(Error::Forbidden);
    }
    relation.valid_time.validate().map_err(Error::Invalid)?;
    if relation.basis.trim().is_empty() {
        return Err(Error::Invalid(
            "A relation needs an evidential basis".into(),
        ));
    }
    memory_version(connection, authority, &relation.from).await?;
    memory_version(connection, authority, &relation.to).await?;
    if relation.kind == RelationKind::DerivedFrom && relation.acceptance != Acceptance::Withdrawn {
        let goal = endpoint(&relation.from);
        let mut pending = VecDeque::from([endpoint(&relation.to)]);
        let mut visited = HashSet::new();
        while let Some(node) = pending.pop_front() {
            if node == goal {
                return Err(Error::Invalid("Derivation would introduce a cycle".into()));
            }
            if !visited.insert(node) {
                continue;
            }
            if visited.len() > 10_000 {
                return Err(Error::LimitExceeded);
            }
            // Cycle checks inspect identities internally, without exposing inaccessible content.
            let rows = sqlx::query("SELECT r.to_id,r.to_revision FROM relation_records r JOIN relation_versions v ON v.tenant_id=r.tenant_id AND v.id=r.id AND v.recorded_to IS NULL WHERE r.tenant_id=$1 AND r.kind='derived_from' AND r.from_id=$2 AND r.from_revision=$3 AND v.acceptance <> 'withdrawn'")
                .bind(authority.tenant_id.to_string()).bind(node.0.to_string()).bind(i64::from(node.1)).fetch_all(&mut *connection).await?;
            for row in rows {
                pending.push_back((
                    id(row.try_get("to_id")?)?,
                    number(row.try_get("to_revision")?)?,
                ));
            }
        }
    }
    Ok(())
}

pub(crate) async fn insert_relation(
    connection: &mut AnyConnection,
    authority: &Authority,
    relation: RelationDraft,
    recorded: CommitPosition,
) -> Result<RelationVersion> {
    let relation = canonical(relation);
    validate(connection, authority, &relation).await?;
    let tenant = authority.tenant_id.to_string();
    let kind = tag(&relation.kind)?;
    let duplicate = sqlx::query("SELECT id FROM relation_records WHERE tenant_id=$1 AND kind=$2 AND from_id=$3 AND from_revision=$4 AND to_id=$5 AND to_revision=$6")
        .bind(&tenant).bind(&kind).bind(relation.from.memory_id.to_string()).bind(i64::from(relation.from.revision.get())).bind(relation.to.memory_id.to_string()).bind(i64::from(relation.to.revision.get())).fetch_optional(&mut *connection).await?;
    if duplicate.is_some() {
        return Err(Error::Conflict);
    }
    let id = Uuid::now_v7();
    write_scope(connection, authority, id, &relation.scope).await?;
    sqlx::query("INSERT INTO relation_records (tenant_id,id,revision,kind,from_id,from_revision,to_id,to_revision) VALUES ($1,$2,1,$3,$4,$5,$6,$7)")
        .bind(&tenant).bind(id.to_string()).bind(kind).bind(relation.from.memory_id.to_string()).bind(i64::from(relation.from.revision.get())).bind(relation.to.memory_id.to_string()).bind(i64::from(relation.to.revision.get())).execute(&mut *connection).await?;
    write_version(connection, authority, id, 1, relation, recorded).await
}

async fn write_version(
    connection: &mut AnyConnection,
    authority: &Authority,
    id: Uuid,
    revision: u32,
    relation: RelationDraft,
    recorded: CommitPosition,
) -> Result<RelationVersion> {
    let version_id = Uuid::now_v7();
    let (known, from, to) = temporal::interval(&relation.valid_time);
    sqlx::query("INSERT INTO relation_versions (tenant_id,id,revision,version_id,recorded_from,recorded_at,valid_known,valid_from,valid_to,acceptance,data) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)")
        .bind(authority.tenant_id.to_string()).bind(id.to_string()).bind(i64::from(revision)).bind(version_id.to_string()).bind(i64::from(recorded.sequence)).bind(recorded.recorded_at.timestamp_millis()).bind(known).bind(from).bind(to).bind(tag(&relation.acceptance)?).bind(serde_json::to_string(&relation)?).execute(connection).await?;
    Ok(RelationVersion {
        id,
        revision,
        version_id,
        recorded,
        recorded_until: None,
        relation,
    })
}

pub(crate) async fn insert_derivation(
    connection: &mut AnyConnection,
    authority: &Authority,
    from: &MemoryRef,
    to: &MemoryRef,
    scope: &Scope,
    recorded: CommitPosition,
) -> Result<()> {
    insert_relation(
        connection,
        authority,
        RelationDraft {
            from: from.clone(),
            to: to.clone(),
            kind: RelationKind::DerivedFrom,
            scope: scope.clone(),
            basis: "Declared production input".into(),
            evidential_status: EvidentialStatus::Observation,
            acceptance: Acceptance::Accepted,
            valid_time: ValidTime::Unknown,
        },
        recorded,
    )
    .await?;
    Ok(())
}

fn decode_relation(row: &AnyRow) -> Result<RelationVersion> {
    Ok(RelationVersion {
        id: id(row.try_get("id")?)?,
        revision: number(row.try_get("revision")?)?,
        version_id: id(row.try_get("version_id")?)?,
        recorded: CommitPosition {
            sequence: number(row.try_get("recorded_from")?)?,
            recorded_at: timestamp(row.try_get("recorded_at")?)?,
        },
        recorded_until: row
            .try_get::<Option<i64>, _>("recorded_to")?
            .map(number)
            .transpose()?,
        relation: serde_json::from_str(row.try_get("data")?)?,
    })
}

pub(crate) async fn revise_relation_in(
    c: &mut AnyConnection,
    authority: &Authority,
    id: Uuid,
    expected_revision: u32,
    relation: RelationDraft,
    position: CommitPosition,
) -> Result<RelationVersion> {
    read_scope(c, authority, id).await?;
    let old = sqlx::query(
        "SELECT data FROM relation_versions WHERE tenant_id=$1 AND id=$2 AND revision=$3",
    )
    .bind(authority.tenant_id.to_string())
    .bind(id.to_string())
    .bind(i64::from(expected_revision))
    .fetch_optional(&mut *c)
    .await?
    .ok_or(Error::NotFound)?;
    let prior: RelationDraft = serde_json::from_str(old.try_get("data")?)?;
    let relation = canonical(relation);
    if prior.kind != relation.kind
        || endpoint(&prior.from) != endpoint(&relation.from)
        || endpoint(&prior.to) != endpoint(&relation.to)
        || !same_scope(&prior.scope, &relation.scope)
    {
        return Err(Error::Invalid(
            "Changing relation endpoints, kind or scope requires a new relation".into(),
        ));
    }
    validate(c, authority, &relation).await?;
    let revision = expected_revision
        .checked_add(1)
        .ok_or_else(|| Error::Invalid("Revision overflow".into()))?;
    changed(
        sqlx::query(
            "UPDATE relation_records SET revision=$1 WHERE tenant_id=$2 AND id=$3 AND revision=$4",
        )
        .bind(i64::from(revision))
        .bind(authority.tenant_id.to_string())
        .bind(id.to_string())
        .bind(i64::from(expected_revision))
        .execute(&mut *c)
        .await?
        .rows_affected(),
    )?;
    sqlx::query("UPDATE relation_versions SET recorded_to=$1 WHERE tenant_id=$2 AND id=$3 AND recorded_to IS NULL")
            .bind(i64::from(position.sequence)).bind(authority.tenant_id.to_string()).bind(id.to_string()).execute(&mut *c).await?;
    let result = write_version(c, authority, id, revision, relation, position).await?;
    Ok(result)
}
