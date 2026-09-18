use crate::{Error, Result, database::id};
use memory_domain::contracts::{Authority, Scope, SourceRef};
use sqlx::{AnyConnection, Row};
use uuid::Uuid;

/// Builds a scoped SQL predicate. All values are bound parameters; `s` is the
/// resource_scopes alias in repository-owned SQL.
pub(crate) fn predicate(authority: &Authority) -> (String, Vec<String>) {
    let mut values = vec![authority.tenant_id.to_string()];
    let mut sql = "s.tenant_id = $1 AND NOT EXISTS (SELECT 1 FROM revoked_resources r WHERE r.tenant_id=s.tenant_id AND r.resource_id=s.id)".to_owned();
    for (column, value) in [
        ("user_id", authority.scope.user_id),
        ("project_id", authority.scope.project_id),
        ("task_id", authority.scope.task_id),
    ] {
        if let Some(value) = value {
            values.push(value.to_string());
            sql.push_str(&format!(
                " AND (s.{column} IS NULL OR s.{column} = ${})",
                values.len()
            ));
        }
    }
    if !authority.scope.entity_ids.is_empty() {
        let mut parameters = Vec::new();
        for entity in &authority.scope.entity_ids {
            values.push(entity.to_string());
            parameters.push(format!("${}", values.len()));
        }
        sql.push_str(&format!(" AND NOT EXISTS (SELECT 1 FROM scope_entities e WHERE e.tenant_id=s.tenant_id AND e.scope_id=s.id AND e.entity_id NOT IN ({}))", parameters.join(",")));
    }
    if !authority.scope.source_versions.is_empty() {
        let mut alternatives = Vec::new();
        for source in &authority.scope.source_versions {
            values.push(source.source_id.to_string());
            let source_index = values.len();
            values.push(source.revision.clone());
            alternatives.push(format!(
                "(v.source_id=${source_index} AND v.revision=${})",
                values.len()
            ));
        }
        sql.push_str(&format!(" AND NOT EXISTS (SELECT 1 FROM scope_sources v WHERE v.tenant_id=s.tenant_id AND v.scope_id=s.id AND NOT ({}))", alternatives.join(" OR ")));
    }
    (sql, values)
}

pub(crate) async fn read_scope(
    connection: &mut AnyConnection,
    authority: &Authority,
    scope_id: Uuid,
) -> Result<Scope> {
    let (predicate, parameters) = predicate(authority);
    let sql = format!(
        "SELECT s.user_id, s.project_id, s.task_id FROM resource_scopes s WHERE {predicate} AND s.id = ${}",
        parameters.len() + 1
    );
    let mut query = sqlx::query(&sql);
    for parameter in parameters {
        query = query.bind(parameter);
    }
    let row = query
        .bind(scope_id.to_string())
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(Error::NotFound)?;
    let optional_id = |name| -> Result<Option<Uuid>> {
        row.try_get::<Option<String>, _>(name)?
            .map(|value| id(&value))
            .transpose()
    };
    let entities = sqlx::query("SELECT entity_id FROM scope_entities WHERE tenant_id=$1 AND scope_id=$2 ORDER BY entity_id")
        .bind(authority.tenant_id.to_string()).bind(scope_id.to_string()).fetch_all(&mut *connection).await?;
    let sources = sqlx::query("SELECT source_id, revision FROM scope_sources WHERE tenant_id=$1 AND scope_id=$2 ORDER BY source_id, revision")
        .bind(authority.tenant_id.to_string()).bind(scope_id.to_string()).fetch_all(&mut *connection).await?;
    Ok(Scope {
        user_id: optional_id("user_id")?,
        project_id: optional_id("project_id")?,
        task_id: optional_id("task_id")?,
        entity_ids: entities
            .iter()
            .map(|row| id(row.try_get("entity_id")?))
            .collect::<Result<_>>()?,
        source_versions: sources
            .iter()
            .map(|row| {
                Ok(SourceRef {
                    source_id: id(row.try_get("source_id")?)?,
                    revision: row.try_get("revision")?,
                })
            })
            .collect::<Result<_>>()?,
    })
}

pub(crate) async fn write_scope(
    connection: &mut AnyConnection,
    authority: &Authority,
    scope_id: Uuid,
    scope: &Scope,
) -> Result<()> {
    if !authority.scope.permits(scope) {
        return Err(Error::Forbidden);
    }
    crate::retention::check_resource(connection, authority, scope_id).await?;
    let tenant = authority.tenant_id.to_string();
    let scope_id = scope_id.to_string();
    sqlx::query("INSERT INTO resource_scopes (tenant_id,id,user_id,project_id,task_id) VALUES ($1,$2,$3,$4,$5)")
        .bind(&tenant).bind(&scope_id).bind(scope.user_id.map(|v| v.to_string())).bind(scope.project_id.map(|v| v.to_string())).bind(scope.task_id.map(|v| v.to_string())).execute(&mut *connection).await?;
    for entity in &scope.entity_ids {
        sqlx::query("INSERT INTO scope_entities (tenant_id,scope_id,entity_id) VALUES ($1,$2,$3) ON CONFLICT DO NOTHING")
            .bind(&tenant).bind(&scope_id).bind(entity.to_string()).execute(&mut *connection).await?;
    }
    for source in &scope.source_versions {
        if source.revision.trim().is_empty() {
            return Err(Error::Invalid("Source revision is empty".into()));
        }
        sqlx::query("INSERT INTO scope_sources (tenant_id,scope_id,source_id,revision) VALUES ($1,$2,$3,$4) ON CONFLICT DO NOTHING")
            .bind(&tenant).bind(&scope_id).bind(source.source_id.to_string()).bind(&source.revision).execute(&mut *connection).await?;
    }
    Ok(())
}

pub(crate) fn same_scope(left: &Scope, right: &Scope) -> bool {
    left.permits(right) && right.permits(left)
}

/// Shared context can be read under a narrower assignment. Mutating it requires
/// a grant covering the complete owning scope.
pub(crate) async fn require_write_scope(
    connection: &mut AnyConnection,
    authority: &Authority,
    scope_id: Uuid,
) -> Result<()> {
    let scope = read_scope(connection, authority, scope_id).await?;
    if !authority.scope.permits(&scope) {
        return Err(Error::Forbidden);
    }
    Ok(())
}
