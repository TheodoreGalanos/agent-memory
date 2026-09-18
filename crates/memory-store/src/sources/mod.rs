mod adapters;
mod repository;
pub use adapters::{DocumentAdapter, ModelAdapter, SourceAdapter, TableAdapter, ToolEventAdapter};
pub(crate) use repository::{locator, source_version};

use crate::{
    Error, Result, Store,
    artifacts::{ArtifactService, MAX_READ_BYTES},
};
use memory_domain::{
    contracts::{Authority, Origin, SourceRef},
    sources::{ArtifactSpec, Locator, SourceKind, SourceLocator, SourceVersion},
};
use serde_json::Value;
use tokio::io::AsyncReadExt;

/// A bounded read of a retained source revision, with the locator kept alongside it.
#[derive(Debug)]
pub struct SourceRead {
    pub source: SourceRef,
    pub locator: Locator,
    pub content: Value,
}

pub struct SourceService {
    store: Store,
    artifacts: ArtifactService,
    max_snapshot_bytes: u32,
}

impl SourceService {
    pub fn new(store: Store, artifacts: ArtifactService, max_snapshot_bytes: u32) -> Result<Self> {
        if max_snapshot_bytes == 0 {
            return Err(Error::Invalid("Source size limit must be positive".into()));
        }
        Ok(Self {
            store,
            artifacts,
            max_snapshot_bytes,
        })
    }

    pub async fn ingest(
        &self,
        authority: &Authority,
        mut source: SourceVersion,
        adapter: &impl SourceAdapter,
        input: &[u8],
    ) -> Result<SourceVersion> {
        if input.len() > self.max_snapshot_bytes as usize {
            return Err(Error::LimitExceeded);
        }
        if source.kind != adapter.kind() {
            return Err(Error::Invalid(
                "Source kind differs from its adapter".into(),
            ));
        }
        let normalized = adapter.normalize(input)?;
        let expected_bytes = u32::try_from(normalized.len()).map_err(|_| Error::LimitExceeded)?;
        if expected_bytes > self.max_snapshot_bytes {
            return Err(Error::LimitExceeded);
        }
        let artifact = self
            .artifacts
            .allocate(
                authority,
                ArtifactSpec {
                    label: source.label.clone(),
                    scope: source.scope.clone(),
                    media_type: adapter.media_type().into(),
                    expected_bytes,
                    origin: Origin::Observed,
                    retention_class: "source-snapshot".into(),
                    dependencies: vec![],
                },
            )
            .await?;
        let ready = self
            .artifacts
            .upload(
                authority,
                artifact.id,
                artifact.revision,
                normalized.as_slice(),
            )
            .await?;
        source.snapshot_artifact = Some(ready.id);
        self.store.register_source_version(authority, source).await
    }

    /// Paths are supplied by a trusted connector, not by a model-facing read API.
    pub async fn ingest_file(
        &self,
        authority: &Authority,
        source: SourceVersion,
        adapter: &impl SourceAdapter,
        path: impl AsRef<std::path::Path>,
    ) -> Result<SourceVersion> {
        let file = tokio::fs::File::open(path).await?;
        let mut bytes = Vec::new();
        file.take(u64::from(self.max_snapshot_bytes) + 1)
            .read_to_end(&mut bytes)
            .await?;
        self.ingest(authority, source, adapter, &bytes).await
    }

    pub async fn read(
        &self,
        authority: &Authority,
        reference: &SourceRef,
        locator: &Locator,
        max_output_bytes: u32,
    ) -> Result<SourceRead> {
        let source = self.store.source_version(authority, reference).await?;
        let snapshot = source.snapshot_artifact.ok_or(Error::Unavailable)?;
        let artifact = self.artifacts.inspect(authority, snapshot).await?;
        if artifact.spec.expected_bytes > self.max_snapshot_bytes {
            return Err(Error::LimitExceeded);
        }
        let mut bytes = Vec::new();
        let mut offset = 0;
        // Empty snapshots still go through the artifact availability check.
        self.artifacts.read(authority, snapshot, 0..0).await?;
        while offset < artifact.spec.expected_bytes {
            let end = offset
                .saturating_add(MAX_READ_BYTES)
                .min(artifact.spec.expected_bytes);
            bytes.extend_from_slice(
                &self
                    .artifacts
                    .read(authority, snapshot, offset..end)
                    .await?,
            );
            offset = end;
        }
        let adapter: &dyn SourceAdapter = match source.kind {
            SourceKind::Document => &DocumentAdapter,
            SourceKind::Table => &TableAdapter,
            SourceKind::Model => &ModelAdapter,
            SourceKind::ToolEvents => &ToolEventAdapter,
        };
        let content = adapter.read(&bytes, locator)?;
        if serde_json::to_vec(&content)?.len() > max_output_bytes as usize {
            return Err(Error::LimitExceeded);
        }
        Ok(SourceRead {
            source: reference.clone(),
            locator: locator.clone(),
            content,
        })
    }

    pub async fn read_locator(
        &self,
        authority: &Authority,
        id: uuid::Uuid,
        max_output_bytes: u32,
    ) -> Result<SourceRead> {
        let stored: SourceLocator = self.store.source_locator(authority, id).await?;
        self.read(authority, &stored.source, &stored.locator, max_output_bytes)
            .await
    }
}
