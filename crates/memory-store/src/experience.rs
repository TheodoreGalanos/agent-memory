//! ABOUTME: The durable link from a settled task job to the experience source its worker published, which
//! ABOUTME: later schedules formation over ordinary task evidence without any caller authoring candidates.
use crate::{
    Error, Result, Store,
    access::read_scope,
    coordinator::{events::append_event, fence},
};
use memory_domain::{
    contracts::{Authority, SourceRef},
    coordination::Fence,
};
use sqlx::Row;
use uuid::Uuid;

impl Store {
    /// Records the experience source of the fenced job's current attempt and appends the durable event.
    pub async fn record_experience(
        &self,
        authority: &Authority,
        permit: &Fence,
        source: &SourceRef,
    ) -> Result<()> {
        let (mut tx, position) = self.begin_write().await?;
        let job = fence(&mut tx, authority, permit, position.recorded_at).await?;
        sqlx::query(
            "INSERT INTO job_experiences (tenant_id,job_id,source_id,revision,published_at) VALUES ($1,$2,$3,$4,$5) \
             ON CONFLICT (tenant_id,job_id) DO UPDATE SET source_id=excluded.source_id,revision=excluded.revision,published_at=excluded.published_at,formation_job_id=NULL",
        )
        .bind(authority.tenant_id.to_string())
        .bind(job.id.to_string())
        .bind(source.source_id.to_string())
        .bind(&source.revision)
        .bind(position.recorded_at.timestamp_millis())
        .execute(&mut *tx)
        .await?;
        append_event(
            &mut tx,
            authority,
            job.id,
            "experience_published",
            position,
            job.spec.retain_until,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// The experience source published for a job, if any.
    pub async fn experience(
        &self,
        authority: &Authority,
        job_id: Uuid,
    ) -> Result<Option<SourceRef>> {
        let mut connection = self.pool.acquire().await?;
        read_scope(&mut connection, authority, job_id).await?;
        let row = sqlx::query(
            "SELECT source_id,revision FROM job_experiences WHERE tenant_id=$1 AND job_id=$2",
        )
        .bind(authority.tenant_id.to_string())
        .bind(job_id.to_string())
        .fetch_optional(&mut *connection)
        .await?;
        row.map(|row| {
            Ok(SourceRef {
                source_id: crate::database::id(row.try_get("source_id")?)?,
                revision: row.try_get("revision")?,
            })
        })
        .transpose()
        .map_err(|e: Error| e)
    }
}
