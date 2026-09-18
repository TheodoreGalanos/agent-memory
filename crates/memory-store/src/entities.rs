use crate::{
    Error, Result, Store,
    access::{predicate, read_scope, require_write_scope, write_scope},
    database::{changed, from_tag, id, number, tag},
};
use memory_domain::{
    contracts::Authority,
    sources::{Entity, EntityLink},
};
use sqlx::{AnyConnection, Row};
use uuid::Uuid;

impl Store {
    pub async fn create_entity(&self, authority: &Authority, entity: Entity) -> Result<Entity> {
        if !authority.scope.entity_ids.is_empty()
            && !authority.scope.entity_ids.contains(&entity.id)
        {
            return Err(Error::Forbidden);
        }
        if entity.label.trim().is_empty()
            || entity.native_id.trim().is_empty()
            || entity.provider.trim().is_empty()
        {
            return Err(Error::Invalid(
                "Entity identity and label are required".into(),
            ));
        }
        let (mut tx, _) = self.begin_write().await?;
        write_scope(&mut tx, authority, entity.id, &entity.scope).await?;
        sqlx::query(
            "INSERT INTO entities (tenant_id,id,provider,native_id,label) VALUES ($1,$2,$3,$4,$5)",
        )
        .bind(authority.tenant_id.to_string())
        .bind(entity.id.to_string())
        .bind(&entity.provider)
        .bind(&entity.native_id)
        .bind(&entity.label)
        .execute(&mut *tx)
        .await?;
        for alias in &entity.aliases {
            if alias.trim().is_empty() {
                return Err(Error::Invalid("Entity alias is empty".into()));
            }
            sqlx::query("INSERT INTO entity_aliases (tenant_id,entity_id,alias) VALUES ($1,$2,$3) ON CONFLICT DO NOTHING").bind(authority.tenant_id.to_string()).bind(entity.id.to_string()).bind(alias).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(entity)
    }

    pub async fn entity(&self, authority: &Authority, id: Uuid) -> Result<Entity> {
        entity(&mut *self.pool.acquire().await?, authority, id).await
    }

    pub async fn find_entities(&self, authority: &Authority, name: &str) -> Result<Vec<Entity>> {
        let (predicate, mut values) = predicate(authority);
        values.push(name.into());
        let n = values.len();
        let mut sql = format!(
            "SELECT e.id FROM entities e JOIN resource_scopes s ON s.tenant_id=e.tenant_id AND s.id=e.id WHERE {predicate} AND (e.native_id=${n} OR e.label=${n} OR EXISTS (SELECT 1 FROM entity_aliases a WHERE a.tenant_id=e.tenant_id AND a.entity_id=e.id AND a.alias=${n}))"
        );
        if !authority.scope.entity_ids.is_empty() {
            let mut parameters = Vec::new();
            for entity in &authority.scope.entity_ids {
                values.push(entity.to_string());
                parameters.push(format!("${}", values.len()));
            }
            sql.push_str(&format!(" AND e.id IN ({})", parameters.join(",")));
        }
        sql.push_str(" ORDER BY e.id LIMIT 100");
        let mut query = sqlx::query(&sql);
        for value in values {
            query = query.bind(value);
        }
        let mut connection = self.pool.acquire().await?;
        let rows = query.fetch_all(&mut *connection).await?;
        let mut entities = Vec::new();
        for row in rows {
            entities.push(entity(&mut connection, authority, id(row.try_get("id")?)?).await?);
        }
        Ok(entities)
    }

    pub async fn save_entity_link(
        &self,
        authority: &Authority,
        mut link: EntityLink,
        expected_revision: Option<u32>,
    ) -> Result<EntityLink> {
        if link.basis.trim().is_empty() {
            return Err(Error::Invalid("An entity link needs a basis".into()));
        }
        let (mut tx, _) = self.begin_write().await?;
        entity(&mut tx, authority, link.from).await?;
        entity(&mut tx, authority, link.to).await?;
        require_write_scope(&mut tx, authority, link.from).await?;
        require_write_scope(&mut tx, authority, link.to).await?;
        if let Some(expected) = expected_revision {
            link.revision = expected
                .checked_add(1)
                .ok_or_else(|| Error::Invalid("Revision overflow".into()))?;
            changed(sqlx::query("UPDATE entity_links SET revision=$1,acceptance=$2,basis=$3 WHERE tenant_id=$4 AND id=$5 AND revision=$6 AND from_id=$7 AND to_id=$8")
                .bind(i64::from(link.revision)).bind(tag(&link.acceptance)?).bind(&link.basis).bind(authority.tenant_id.to_string()).bind(link.id.to_string()).bind(i64::from(expected)).bind(link.from.to_string()).bind(link.to.to_string()).execute(&mut *tx).await?.rows_affected())?;
        } else {
            link.revision = 1;
            sqlx::query("INSERT INTO entity_links (tenant_id,id,revision,from_id,to_id,acceptance,basis) VALUES ($1,$2,1,$3,$4,$5,$6)")
                .bind(authority.tenant_id.to_string()).bind(link.id.to_string()).bind(link.from.to_string()).bind(link.to.to_string()).bind(tag(&link.acceptance)?).bind(&link.basis).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(link)
    }

    pub async fn entity_link(&self, authority: &Authority, link_id: Uuid) -> Result<EntityLink> {
        let mut connection = self.pool.acquire().await?;
        let row = sqlx::query("SELECT * FROM entity_links WHERE tenant_id=$1 AND id=$2")
            .bind(authority.tenant_id.to_string())
            .bind(link_id.to_string())
            .fetch_optional(&mut *connection)
            .await?
            .ok_or(Error::NotFound)?;
        let from = id(row.try_get("from_id")?)?;
        let to = id(row.try_get("to_id")?)?;
        entity(&mut connection, authority, from).await?;
        entity(&mut connection, authority, to).await?;
        Ok(EntityLink {
            id: link_id,
            revision: number(row.try_get("revision")?)?,
            from,
            to,
            acceptance: from_tag(row.try_get("acceptance")?)?,
            basis: row.try_get("basis")?,
        })
    }
}

pub(crate) async fn entity(
    connection: &mut AnyConnection,
    authority: &Authority,
    entity_id: Uuid,
) -> Result<Entity> {
    if !authority.scope.entity_ids.is_empty() && !authority.scope.entity_ids.contains(&entity_id) {
        return Err(Error::NotFound);
    }
    let scope = read_scope(connection, authority, entity_id).await?;
    let row =
        sqlx::query("SELECT provider,native_id,label FROM entities WHERE tenant_id=$1 AND id=$2")
            .bind(authority.tenant_id.to_string())
            .bind(entity_id.to_string())
            .fetch_optional(&mut *connection)
            .await?
            .ok_or(Error::NotFound)?;
    let aliases = sqlx::query(
        "SELECT alias FROM entity_aliases WHERE tenant_id=$1 AND entity_id=$2 ORDER BY alias",
    )
    .bind(authority.tenant_id.to_string())
    .bind(entity_id.to_string())
    .fetch_all(connection)
    .await?;
    Ok(Entity {
        id: entity_id,
        scope,
        provider: row.try_get("provider")?,
        native_id: row.try_get("native_id")?,
        label: row.try_get("label")?,
        aliases: aliases
            .iter()
            .map(|row| row.try_get("alias"))
            .collect::<std::result::Result<_, _>>()?,
    })
}
