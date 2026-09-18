use crate::{
    Error, Result, Store,
    access::{read_scope, require_write_scope, write_scope},
    database::{changed, from_tag, number},
};
use bytes::Bytes;
use memory_domain::{
    contracts::Authority,
    sources::{Artifact, ArtifactSpec, ArtifactState},
};
use object_store::{
    ObjectStore, ObjectStoreExt, WriteMultipart, local::LocalFileSystem, path::Path,
};
use sqlx::{AnyConnection, Row};
use std::{collections::HashSet, ops::Range, sync::Arc};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;

pub const MAX_READ_BYTES: u32 = 1024 * 1024;

#[derive(Clone)]
pub struct ArtifactService {
    store: Store,
    objects: Arc<dyn ObjectStore>,
}

pub struct ArtifactUpload {
    writer: WriteMultipart,
    ticket: UploadTicket,
    written: u32,
}
pub struct CompletedUpload {
    ticket: UploadTicket,
}
struct UploadTicket {
    id: Uuid,
    attempt: Uuid,
    revision: u32,
    expected: u32,
    tenant: Uuid,
}

impl ArtifactUpload {
    pub async fn write(&mut self, bytes: &[u8]) -> Result<()> {
        let len = u32::try_from(bytes.len()).map_err(|_| Error::LimitExceeded)?;
        let total = self.written.checked_add(len).ok_or(Error::LimitExceeded)?;
        if total > self.ticket.expected {
            return Err(Error::LimitExceeded);
        }
        for chunk in bytes.chunks(64 * 1024) {
            self.writer.wait_for_capacity(2).await?;
            self.writer.write(chunk);
        }
        self.written = total;
        Ok(())
    }

    pub async fn complete(self) -> Result<CompletedUpload> {
        if self.written != self.ticket.expected {
            self.writer.abort().await?;
            return Err(Error::Invalid(
                "Upload ended before its declared length".into(),
            ));
        }
        self.writer.finish().await?;
        Ok(CompletedUpload {
            ticket: self.ticket,
        })
    }

    pub async fn abort(self) -> Result<()> {
        self.writer.abort().await?;
        Ok(())
    }
}

impl ArtifactService {
    pub fn new(store: Store, objects: Arc<dyn ObjectStore>) -> Self {
        Self { store, objects }
    }

    pub async fn local(store: Store, root: impl AsRef<std::path::Path>) -> Result<Self> {
        tokio::fs::create_dir_all(&root).await?;
        let objects = LocalFileSystem::new_with_prefix(root)?.with_fsync(true);
        Ok(Self::new(store, Arc::new(objects)))
    }

    pub async fn allocate(&self, authority: &Authority, spec: ArtifactSpec) -> Result<Artifact> {
        self.allocate_named(authority, Uuid::now_v7(), spec).await
    }

    /// A caller-owned identity lets interrupted result publication recover its artifact.
    pub async fn allocate_named(
        &self,
        authority: &Authority,
        id: Uuid,
        spec: ArtifactSpec,
    ) -> Result<Artifact> {
        if spec.label.trim().is_empty()
            || spec.media_type.trim().is_empty()
            || spec.retention_class.trim().is_empty()
        {
            return Err(Error::Invalid(
                "Artifact label, media type and retention class are required".into(),
            ));
        }
        let (mut tx, _) = self.store.begin_write().await?;
        for dependency in &spec.dependencies {
            ready_artifact(&mut tx, authority, *dependency).await?;
        }
        if sqlx::query("SELECT id FROM artifact_records WHERE tenant_id=$1 AND id=$2")
            .bind(authority.tenant_id.to_string())
            .bind(id.to_string())
            .fetch_optional(&mut *tx)
            .await?
            .is_some()
        {
            require_write_scope(&mut tx, authority, id).await?;
            let prior = artifact(&mut tx, authority, id).await?.0;
            if serde_json::to_value(&prior.spec)? != serde_json::to_value(&spec)? {
                return Err(Error::Conflict);
            }
            return Ok(prior);
        }
        write_scope(&mut tx, authority, id, &spec.scope).await?;
        sqlx::query("INSERT INTO artifact_records (tenant_id,id,revision,state,spec) VALUES ($1,$2,1,'pending',$3)").bind(authority.tenant_id.to_string()).bind(id.to_string()).bind(serde_json::to_string(&spec)?).execute(&mut *tx).await?;
        for dependency in &spec.dependencies {
            sqlx::query("INSERT INTO artifact_dependencies (tenant_id,artifact_id,depends_on) VALUES ($1,$2,$3) ON CONFLICT DO NOTHING").bind(authority.tenant_id.to_string()).bind(id.to_string()).bind(dependency.to_string()).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(Artifact {
            id,
            revision: 1,
            state: ArtifactState::Pending,
            spec,
        })
    }

    pub async fn inspect(&self, authority: &Authority, id: Uuid) -> Result<Artifact> {
        Ok(
            artifact(&mut *self.store.pool.acquire().await?, authority, id)
                .await?
                .0,
        )
    }

    pub async fn begin_upload(
        &self,
        authority: &Authority,
        id: Uuid,
        expected_revision: u32,
    ) -> Result<ArtifactUpload> {
        let (mut tx, _) = self.store.begin_write().await?;
        require_write_scope(&mut tx, authority, id).await?;
        let (artifact, _, _) = artifact(&mut tx, authority, id).await?;
        let attempt = Uuid::now_v7();
        let key = format!("{}/{id}/{attempt}", authority.tenant_id);
        let revision = expected_revision
            .checked_add(1)
            .ok_or_else(|| Error::Invalid("Revision overflow".into()))?;
        changed(sqlx::query("UPDATE artifact_records SET state='uploading',revision=$1,attempt_id=$2,object_key=$3 WHERE tenant_id=$4 AND id=$5 AND revision=$6 AND state='pending'")
            .bind(i64::from(revision)).bind(attempt.to_string()).bind(&key).bind(authority.tenant_id.to_string()).bind(id.to_string()).bind(i64::from(expected_revision)).execute(&mut *tx).await?.rows_affected())?;
        tx.commit().await?;
        let writer = WriteMultipart::new(self.objects.put_multipart(&Path::from(key)).await?);
        Ok(ArtifactUpload {
            writer,
            ticket: UploadTicket {
                id,
                attempt,
                revision,
                expected: artifact.spec.expected_bytes,
                tenant: authority.tenant_id,
            },
            written: 0,
        })
    }

    /// The completed object is durable before its database row becomes readable.
    pub async fn publish(
        &self,
        authority: &Authority,
        completed: CompletedUpload,
    ) -> Result<Artifact> {
        let ticket = completed.ticket;
        if ticket.tenant != authority.tenant_id {
            return Err(Error::NotFound);
        }
        let (mut tx, _) = self.store.begin_write().await?;
        require_write_scope(&mut tx, authority, ticket.id).await?;
        let (mut artifact, _, _) = artifact(&mut tx, authority, ticket.id).await?;
        for dependency in &artifact.spec.dependencies {
            ready_artifact(&mut tx, authority, *dependency).await?;
        }
        changed(sqlx::query("UPDATE artifact_records SET state='ready',revision=revision+1 WHERE tenant_id=$1 AND id=$2 AND revision=$3 AND attempt_id=$4 AND state='uploading'")
            .bind(authority.tenant_id.to_string()).bind(ticket.id.to_string()).bind(i64::from(ticket.revision)).bind(ticket.attempt.to_string()).execute(&mut *tx).await?.rows_affected())?;
        tx.commit().await?;
        artifact.state = ArtifactState::Ready;
        artifact.revision += 1;
        Ok(artifact)
    }

    pub async fn upload(
        &self,
        authority: &Authority,
        id: Uuid,
        expected_revision: u32,
        mut reader: impl AsyncRead + Unpin,
    ) -> Result<Artifact> {
        let mut upload = self.begin_upload(authority, id, expected_revision).await?;
        let mut buffer = vec![0; 64 * 1024];
        loop {
            let length = match reader.read(&mut buffer).await {
                Ok(length) => length,
                Err(error) => {
                    upload.abort().await?;
                    return Err(error.into());
                }
            };
            if length == 0 {
                break;
            }
            if let Err(error) = upload.write(&buffer[..length]).await {
                upload.abort().await?;
                return Err(error);
            }
        }
        self.publish(authority, upload.complete().await?).await
    }

    /// Call after stopping the previous uploader. A new attempt gets its own key;
    /// a late completion cannot publish over a replacement attempt.
    pub async fn recover_upload(
        &self,
        authority: &Authority,
        id: Uuid,
        expected_revision: u32,
    ) -> Result<Artifact> {
        let (current, key, attempt) =
            artifact(&mut *self.store.pool.acquire().await?, authority, id).await?;
        if !authority.scope.permits(&current.spec.scope) {
            return Err(Error::Forbidden);
        }
        if current.revision != expected_revision || current.state != ArtifactState::Uploading {
            return Err(Error::Conflict);
        }
        let key = key.ok_or(Error::Unavailable)?;
        match self.objects.head(&Path::from(key)).await {
            Ok(meta) if meta.size == u64::from(current.spec.expected_bytes) => {
                self.publish(
                    authority,
                    CompletedUpload {
                        ticket: UploadTicket {
                            id,
                            attempt: attempt.ok_or(Error::Unavailable)?,
                            revision: current.revision,
                            expected: current.spec.expected_bytes,
                            tenant: authority.tenant_id,
                        },
                    },
                )
                .await
            }
            Ok(_) => Err(Error::Unavailable),
            Err(object_store::Error::NotFound { .. }) => {
                let (mut tx, _) = self.store.begin_write().await?;
                require_write_scope(&mut tx, authority, id).await?;
                changed(sqlx::query("UPDATE artifact_records SET state='pending',revision=revision+1,object_key=NULL,attempt_id=NULL WHERE tenant_id=$1 AND id=$2 AND revision=$3 AND state='uploading'")
                    .bind(authority.tenant_id.to_string()).bind(id.to_string()).bind(i64::from(expected_revision)).execute(&mut *tx).await?.rows_affected())?;
                tx.commit().await?;
                self.inspect(authority, id).await
            }
            Err(error) => Err(error.into()),
        }
    }

    pub async fn read(&self, authority: &Authority, id: Uuid, range: Range<u32>) -> Result<Bytes> {
        if range.start > range.end || range.end - range.start > MAX_READ_BYTES {
            return Err(Error::LimitExceeded);
        }
        let (artifact, key) =
            ready_artifact(&mut *self.store.pool.acquire().await?, authority, id).await?;
        if range.end > artifact.spec.expected_bytes {
            return Err(Error::Invalid("Read extends beyond the artifact".into()));
        }
        if range.is_empty() {
            return match self.objects.head(&Path::from(key)).await {
                Ok(meta) if meta.size == u64::from(artifact.spec.expected_bytes) => {
                    Ok(Bytes::new())
                }
                Ok(_) | Err(object_store::Error::NotFound { .. }) => Err(Error::Unavailable),
                Err(error) => Err(error.into()),
            };
        }
        match self
            .objects
            .get_range(
                &Path::from(key),
                u64::from(range.start)..u64::from(range.end),
            )
            .await
        {
            Ok(bytes) if bytes.len() == (range.end - range.start) as usize => {
                ready_artifact(&mut *self.store.pool.acquire().await?, authority, id).await?;
                Ok(bytes)
            }
            Ok(_) => Err(Error::Unavailable),
            Err(object_store::Error::NotFound { .. }) => Err(Error::Unavailable),
            Err(error) => Err(error.into()),
        }
    }

    pub async fn export(
        &self,
        authority: &Authority,
        id: Uuid,
        mut output: impl AsyncWrite + Unpin,
    ) -> Result<u32> {
        let artifact = self.inspect(authority, id).await?;
        let mut offset = 0;
        // Even an empty export must validate current availability and dependencies.
        self.read(authority, id, 0..0).await?;
        while offset < artifact.spec.expected_bytes {
            let end = offset
                .saturating_add(MAX_READ_BYTES)
                .min(artifact.spec.expected_bytes);
            output
                .write_all(&self.read(authority, id, offset..end).await?)
                .await?;
            offset = end;
        }
        output.flush().await?;
        Ok(offset)
    }

    /// Caller must first stop the session/sandbox containers listed by the deletion report.
    /// Objects are removed before SQL metadata, so retrying a failed purge retains their keys.
    pub async fn purge_deletion(
        &self,
        authority: &Authority,
        deletion: Uuid,
    ) -> Result<memory_domain::retention::DeletionReport> {
        use memory_domain::retention::RetentionBoundary;
        let report = self.store.deletion_report(authority, deletion).await?;
        if report.obligations.iter().any(|o| {
            matches!(
                o.kind,
                RetentionBoundary::Session | RetentionBoundary::Sandbox | RetentionBoundary::Upload
            ) && !o.acknowledged
        }) {
            return Err(Error::Conflict);
        }
        for resource in &report.resources {
            let prefix = Path::from(format!("{}/{resource}/", authority.tenant_id));
            let objects = self.objects.list_with_delimiter(Some(&prefix)).await?;
            for object in objects.objects {
                match self.objects.delete(&object.location).await {
                    Ok(()) | Err(object_store::Error::NotFound { .. }) => (),
                    Err(error) => return Err(error.into()),
                }
            }
        }
        self.store.purge_deletion_rows(authority, deletion).await
    }

    pub async fn revoke(
        &self,
        authority: &Authority,
        id: Uuid,
        expected_revision: u32,
    ) -> Result<()> {
        let (mut tx, _) = self.store.begin_write().await?;
        require_write_scope(&mut tx, authority, id).await?;
        changed(sqlx::query("UPDATE artifact_records SET state='revoked',revision=revision+1 WHERE tenant_id=$1 AND id=$2 AND revision=$3")
            .bind(authority.tenant_id.to_string()).bind(id.to_string()).bind(i64::from(expected_revision)).execute(&mut *tx).await?.rows_affected())?;
        tx.commit().await?;
        Ok(())
    }
}

pub(crate) async fn artifact(
    connection: &mut AnyConnection,
    authority: &Authority,
    id: Uuid,
) -> Result<(Artifact, Option<String>, Option<Uuid>)> {
    read_scope(connection, authority, id).await?;
    let row = sqlx::query("SELECT revision,state,spec,object_key,attempt_id FROM artifact_records WHERE tenant_id=$1 AND id=$2").bind(authority.tenant_id.to_string()).bind(id.to_string()).fetch_optional(connection).await?.ok_or(Error::NotFound)?;
    Ok((
        Artifact {
            id,
            revision: number(row.try_get("revision")?)?,
            state: from_tag(row.try_get("state")?)?,
            spec: serde_json::from_str(row.try_get("spec")?)?,
        },
        row.try_get("object_key")?,
        row.try_get::<Option<String>, _>("attempt_id")?
            .map(|value| crate::database::id(&value))
            .transpose()?,
    ))
}

pub(crate) async fn ready_artifact(
    connection: &mut AnyConnection,
    authority: &Authority,
    id: Uuid,
) -> Result<(Artifact, String)> {
    let mut pending = vec![id];
    let mut visited = HashSet::new();
    let mut root = None;
    while let Some(next) = pending.pop() {
        if !visited.insert(next) {
            continue;
        }
        if visited.len() > 10_000 {
            return Err(Error::LimitExceeded);
        }
        let (artifact, key, _) = artifact(connection, authority, next).await?;
        if artifact.state != ArtifactState::Ready {
            return Err(Error::Unavailable);
        }
        pending.extend(&artifact.spec.dependencies);
        if next == id {
            root = Some((artifact, key.ok_or(Error::Unavailable)?));
        }
    }
    root.ok_or(Error::Unavailable)
}
