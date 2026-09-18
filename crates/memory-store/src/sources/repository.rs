use crate::{
    Error, Result, Store,
    access::{read_scope, write_scope},
    artifacts::ready_artifact,
    database::{from_tag, id, tag, timestamp},
};
use memory_domain::{
    contracts::{Authority, SourceRef},
    sources::{SourceLocator, SourceVersion},
};
use sqlx::{AnyConnection, Row};
use uuid::Uuid;

impl Store {
    pub async fn register_source_version(
        &self,
        authority: &Authority,
        source: SourceVersion,
    ) -> Result<SourceVersion> {
        if !authority.scope.permits(&source.scope) || !permits_source(authority, &source.reference)
        {
            return Err(Error::Forbidden);
        }
        if source.reference.revision.trim().is_empty()
            || source.owner.trim().is_empty()
            || source.label.trim().is_empty()
            || source.acquisition_method.trim().is_empty()
        {
            return Err(Error::Invalid(
                "Source revision, label, owner and acquisition method are required".into(),
            ));
        }
        let (mut tx, position) = self.begin_write().await?;
        if let Some(snapshot) = source.snapshot_artifact {
            ready_artifact(&mut tx, authority, snapshot).await?;
        }
        let tenant = authority.tenant_id.to_string();
        let source_id = source.reference.source_id.to_string();
        let existing =
            sqlx::query("SELECT owner,kind,label FROM sources WHERE tenant_id=$1 AND id=$2")
                .bind(&tenant)
                .bind(&source_id)
                .fetch_optional(&mut *tx)
                .await?;
        if let Some(existing) = existing {
            if existing.try_get::<String, _>("label")? != source.label
                || existing.try_get::<String, _>("owner")? != source.owner
                || existing.try_get::<String, _>("kind")? != tag(&source.kind)?
            {
                return Err(Error::Conflict);
            }
        } else {
            sqlx::query(
                "INSERT INTO sources (tenant_id,id,label,kind,owner) VALUES ($1,$2,$3,$4,$5)",
            )
            .bind(&tenant)
            .bind(&source_id)
            .bind(&source.label)
            .bind(tag(&source.kind)?)
            .bind(&source.owner)
            .execute(&mut *tx)
            .await?;
        }
        let exists = sqlx::query("SELECT revision FROM source_versions WHERE tenant_id=$1 AND source_id=$2 AND revision=$3").bind(&tenant).bind(&source_id).bind(&source.reference.revision).fetch_optional(&mut *tx).await?;
        if exists.is_some() {
            return Err(Error::Conflict);
        }
        crate::retention::check_resource(&mut tx, authority, source.reference.source_id).await?;
        let scope_id = Uuid::now_v7();
        write_scope(&mut tx, authority, scope_id, &source.scope).await?;
        sqlx::query("INSERT INTO source_versions (tenant_id,source_id,revision,scope_id,acquired_at,acquisition_method,precedence,snapshot_artifact) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(&tenant).bind(&source_id).bind(&source.reference.revision).bind(scope_id.to_string()).bind(source.acquired_at.timestamp_millis()).bind(&source.acquisition_method).bind(&source.precedence).bind(source.snapshot_artifact.map(|id| id.to_string())).execute(&mut *tx).await?;
        crate::coordinator::events::append_event(
            &mut tx,
            authority,
            scope_id,
            "source_registered",
            position,
            position.recorded_at + chrono::Duration::days(30),
        )
        .await?;
        tx.commit().await?;
        Ok(source)
    }

    pub async fn source_version(
        &self,
        authority: &Authority,
        reference: &SourceRef,
    ) -> Result<SourceVersion> {
        source_version(&mut *self.pool.acquire().await?, authority, reference).await
    }

    pub async fn source_versions(
        &self,
        authority: &Authority,
        source_id: Uuid,
    ) -> Result<Vec<SourceVersion>> {
        let mut connection = self.pool.acquire().await?;
        let (predicate, mut values) = crate::access::predicate(authority);
        values.push(source_id.to_string());
        let mut sql = format!(
            "SELECT v.revision FROM source_versions v JOIN resource_scopes s ON s.tenant_id=v.tenant_id AND s.id=v.scope_id WHERE {predicate} AND v.source_id=${}",
            values.len()
        );
        if !authority.scope.source_versions.is_empty() {
            let mut parameters = Vec::new();
            for reference in &authority.scope.source_versions {
                if reference.source_id == source_id {
                    values.push(reference.revision.clone());
                    parameters.push(format!("${}", values.len()));
                }
            }
            if parameters.is_empty() {
                return Ok(Vec::new());
            }
            sql.push_str(&format!(" AND v.revision IN ({})", parameters.join(",")));
        }
        sql.push_str(" ORDER BY v.acquired_at,v.revision LIMIT 1000");
        let mut query = sqlx::query(&sql);
        for value in values {
            query = query.bind(value);
        }
        let rows = query.fetch_all(&mut *connection).await?;
        let mut versions = Vec::new();
        for row in rows {
            versions.push(
                source_version(
                    &mut connection,
                    authority,
                    &SourceRef {
                        source_id,
                        revision: row.try_get("revision")?,
                    },
                )
                .await?,
            );
        }
        Ok(versions)
    }

    pub async fn create_source_locator(
        &self,
        authority: &Authority,
        locator: SourceLocator,
    ) -> Result<SourceLocator> {
        let (mut tx, _) = self.begin_write().await?;
        source_version(&mut tx, authority, &locator.source).await?;
        sqlx::query("INSERT INTO source_locators (tenant_id,id,source_id,source_revision,locator) VALUES ($1,$2,$3,$4,$5)")
            .bind(authority.tenant_id.to_string()).bind(locator.id.to_string()).bind(locator.source.source_id.to_string()).bind(&locator.source.revision).bind(serde_json::to_string(&locator.locator)?).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(locator)
    }

    pub async fn source_locator(
        &self,
        authority: &Authority,
        locator_id: Uuid,
    ) -> Result<SourceLocator> {
        locator(&mut *self.pool.acquire().await?, authority, locator_id).await
    }
}

pub(crate) async fn source_version(
    connection: &mut AnyConnection,
    authority: &Authority,
    reference: &SourceRef,
) -> Result<SourceVersion> {
    if !permits_source(authority, reference) {
        return Err(Error::NotFound);
    }
    let row = sqlx::query("SELECT v.*,s.label,s.kind,s.owner FROM source_versions v JOIN sources s ON s.tenant_id=v.tenant_id AND s.id=v.source_id WHERE v.tenant_id=$1 AND v.source_id=$2 AND v.revision=$3")
        .bind(authority.tenant_id.to_string()).bind(reference.source_id.to_string()).bind(&reference.revision).fetch_optional(&mut *connection).await?.ok_or(Error::NotFound)?;
    let scope = read_scope(connection, authority, id(row.try_get("scope_id")?)?).await?;
    Ok(SourceVersion {
        reference: reference.clone(),
        label: row.try_get("label")?,
        scope,
        kind: from_tag(row.try_get("kind")?)?,
        owner: row.try_get("owner")?,
        acquired_at: timestamp(row.try_get("acquired_at")?)?,
        acquisition_method: row.try_get("acquisition_method")?,
        precedence: row.try_get("precedence")?,
        snapshot_artifact: row
            .try_get::<Option<String>, _>("snapshot_artifact")?
            .map(|value| id(&value))
            .transpose()?,
    })
}

pub(crate) async fn locator(
    connection: &mut AnyConnection,
    authority: &Authority,
    locator_id: Uuid,
) -> Result<SourceLocator> {
    let row = sqlx::query("SELECT source_id,source_revision,locator FROM source_locators WHERE tenant_id=$1 AND id=$2").bind(authority.tenant_id.to_string()).bind(locator_id.to_string()).fetch_optional(&mut *connection).await?.ok_or(Error::NotFound)?;
    let source = SourceRef {
        source_id: id(row.try_get("source_id")?)?,
        revision: row.try_get("source_revision")?,
    };
    source_version(connection, authority, &source).await?;
    Ok(SourceLocator {
        id: locator_id,
        source,
        locator: serde_json::from_str(row.try_get("locator")?)?,
    })
}

fn permits_source(authority: &Authority, reference: &SourceRef) -> bool {
    authority.scope.source_versions.is_empty()
        || authority.scope.source_versions.contains(reference)
}
