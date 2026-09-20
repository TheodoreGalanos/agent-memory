//! Source, experience and result publication through the artifact store.
use super::{Host, Role, error, map_error};
use memory_domain::{
    contracts::{Authority, ContractError, Process, ReasonCode, SourceRef},
    coordination::Fence,
};
use memory_store::Error;
use uuid::Uuid;

/// Source snapshots accepted over HTTP; formation windows read admitted snapshots of the same size.
/// Bounds one published transcript; the snapshot limit bounds its bytes.
const MAX_EXPERIENCE_EVENTS: usize = 2000;
const MAX_SOURCE_SNAPSHOT_BYTES: u32 = 1_048_576;

impl Host {
    /// Connectors publish source revisions through the adapters for each declared kind.
    /// A worker publishes its attempt's transcript. The source is keyed by job and attempt, scoped to
    /// the brief, and recorded durably so formation can be scheduled from it. Republishing the same
    /// attempt returns the existing source.
    pub(super) async fn publish_experience(
        &self,
        auth: &Authority,
        fence: &Fence,
        events: Vec<memory_domain::sources::ToolEvent>,
    ) -> Result<memory_domain::sources::SourceVersion, Error> {
        use memory_domain::sources::{SourceKind, SourceVersion};
        use memory_store::sources::{SourceService, ToolEventAdapter};
        if events.is_empty() || events.len() > MAX_EXPERIENCE_EVENTS {
            return Err(Error::Invalid(format!(
                "An experience holds 1 to {MAX_EXPERIENCE_EVENTS} events"
            )));
        }
        let job = self.store.assigned_job(auth, fence).await?;
        if job.spec.brief.process != Process::Investigation {
            return Err(Error::Forbidden);
        }
        let reference = SourceRef {
            source_id: job.id,
            revision: job.attempt.to_string(),
        };
        // The task executed the work; the experience belongs to the project so later tasks under the
        // same policy can form from and retrieve it. It is a new source about the task, not one of the
        // task's input source versions, so that restriction does not carry over. Task provenance stays
        // on the job link and label.
        let scope = memory_domain::contracts::Scope {
            task_id: None,
            source_versions: vec![],
            ..job.spec.brief.scope.clone()
        };
        let narrow = Authority {
            scope: scope.clone(),
            ..auth.clone()
        };
        if self.store.experience(auth, job.id).await?.as_ref() == Some(&reference) {
            return self.store.source_version(&narrow, &reference).await;
        }
        let bytes = serde_json::to_vec(&serde_json::json!({
            "schema_version": "memory-tool-events/1",
            "events": events,
        }))?;
        let sources = SourceService::new(
            self.store.clone(),
            self.artifacts.clone(),
            MAX_SOURCE_SNAPSHOT_BYTES,
        )?;
        let source = sources
            .ingest(
                &narrow,
                SourceVersion {
                    reference: reference.clone(),
                    label: format!(
                        "Experience of task {}: {}",
                        job.spec.brief.task_id, job.spec.brief.purpose
                    ),
                    scope,
                    kind: SourceKind::ToolEvents,
                    owner: "pool".into(),
                    acquired_at: chrono::Utc::now(),
                    acquisition_method: "pi-session".into(),
                    precedence: None,
                    snapshot_artifact: None,
                },
                &ToolEventAdapter,
                &bytes,
            )
            .await?;
        self.store
            .record_experience(auth, fence, &reference)
            .await?;
        Ok(source)
    }

    pub(super) async fn ingest_source(
        &self,
        auth: &Authority,
        source: memory_domain::sources::SourceVersion,
        content: serde_json::Value,
    ) -> Result<memory_domain::sources::SourceVersion, Error> {
        use memory_domain::sources::SourceKind;
        use memory_store::sources::{
            DocumentAdapter, ModelAdapter, SourceService, TableAdapter, ToolEventAdapter,
        };
        if source.snapshot_artifact.is_some() {
            return Err(Error::Invalid(
                "The host records the snapshot artifact; do not supply one".into(),
            ));
        }
        let bytes = match content {
            serde_json::Value::String(text) => text.into_bytes(),
            other => serde_json::to_vec(&other)?,
        };
        let sources = SourceService::new(
            self.store.clone(),
            self.artifacts.clone(),
            MAX_SOURCE_SNAPSHOT_BYTES,
        )?;
        let result = match source.kind {
            SourceKind::Document => sources.ingest(auth, source, &DocumentAdapter, &bytes).await,
            SourceKind::Table => sources.ingest(auth, source, &TableAdapter, &bytes).await,
            SourceKind::Model => sources.ingest(auth, source, &ModelAdapter, &bytes).await,
            SourceKind::ToolEvents => {
                sources
                    .ingest(auth, source, &ToolEventAdapter, &bytes)
                    .await
            }
        };
        // Content that the adapter cannot parse is the connector's error, not a host failure.
        result.map_err(|e| match e {
            Error::Json(parse) => {
                Error::Invalid(format!("Source content does not match its kind: {parse}"))
            }
            other => other,
        })
    }
    pub async fn upload_artifact(
        &self,
        authorization: Option<&str>,
        id: Uuid,
        bytes: &[u8],
    ) -> Result<memory_domain::sources::Artifact, ContractError> {
        use memory_domain::sources::ArtifactState;
        let credential = self.credential(authorization)?;
        if !matches!(credential.role, Role::Administrator) {
            return Err(error(
                ReasonCode::ForbiddenScope,
                "Artifact publication requires the trusted bridge role",
            ));
        }
        let artifact = self
            .artifacts
            .inspect(&credential.authority, id)
            .await
            .map_err(map_error)?;
        if bytes.len() != artifact.spec.expected_bytes as usize {
            return Err(error(
                ReasonCode::InvalidPayload,
                "Artifact length does not match its allocation",
            ));
        }
        match artifact.state {
            ArtifactState::Ready => Err(error(
                ReasonCode::RequestConflict,
                "Artifact is already published",
            )),
            ArtifactState::Pending => self
                .artifacts
                .upload(&credential.authority, id, artifact.revision, bytes)
                .await
                .map_err(map_error),
            _ => Err(error(
                ReasonCode::SourceUnavailable,
                "Artifact needs explicit upload recovery or is revoked",
            )),
        }
    }
    pub async fn read_artifact(
        &self,
        authorization: Option<&str>,
        id: Uuid,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<u8>, ContractError> {
        let credential = self.credential(authorization)?;
        if matches!(credential.role, Role::Worker { .. }) {
            return Err(error(
                ReasonCode::ForbiddenScope,
                "Worker artifacts must be supplied through assigned inputs",
            ));
        }
        let end = offset
            .checked_add(limit)
            .ok_or_else(|| error(ReasonCode::InvalidPayload, "Invalid artifact range"))?;
        self.artifacts
            .read(&credential.authority, id, offset..end)
            .await
            .map(|bytes| bytes.to_vec())
            .map_err(map_error)
    }
    pub(super) async fn publish_result(
        &self,
        auth: &Authority,
        request_id: Uuid,
        fence: &memory_domain::coordination::Fence,
        mut result: memory_domain::contracts::WorkResult,
    ) -> memory_store::Result<memory_domain::coordination::Job> {
        use memory_domain::{
            contracts::Origin,
            coordination::Resources,
            sources::{ArtifactSpec, ArtifactState},
        };
        result.validate().map_err(|e| Error::Invalid(e.message))?;
        result.result_artifact = Some(request_id);
        let snapshot = self.store.job(auth, fence.job_id).await?;
        if snapshot.state.terminal() {
            return self
                .store
                .complete_job(auth, request_id, fence, result)
                .await;
        }
        let job = self.store.assigned_job(auth, fence).await?;
        if job.cancel_requested || !job.spec.brief.scope.permits(&result.examined_scope) {
            return Err(Error::Forbidden);
        }
        if result
            .inputs
            .sources
            .iter()
            .any(|r| !job.spec.brief.capabilities.sources.contains(r))
            || result
                .inputs
                .memories
                .iter()
                .any(|r| !job.spec.brief.inputs.memories.contains(r))
        {
            return Err(Error::Forbidden);
        }
        for id in result.inputs.artifacts.iter().chain(&result.child_outputs) {
            self.store.input_artifact(auth, fence, *id).await?;
        }
        let data = serde_json::to_vec(&result)?;
        let bytes = u32::try_from(data.len()).map_err(|_| Error::LimitExceeded)?;
        if bytes > job.spec.brief.limits.max_output_bytes.get()
            || bytes > memory_store::artifacts::MAX_READ_BYTES
        {
            return Err(Error::LimitExceeded);
        }
        let usage = Resources {
            output_bytes: bytes,
            ..Default::default()
        };
        let reservation = self
            .store
            .reserve_usage(auth, fence, &format!("result:{request_id}"), usage, true)
            .await?;
        let artifact = self
            .artifacts
            .allocate_named(
                auth,
                request_id,
                ArtifactSpec {
                    label: format!("Job {} result", job.id),
                    scope: job.spec.brief.scope.clone(),
                    media_type: "application/json".into(),
                    expected_bytes: bytes,
                    origin: Origin::AgentGenerated,
                    retention_class: job.spec.brief.retention_policy.clone(),
                    dependencies: result.inputs.artifacts.clone(),
                },
            )
            .await?;
        match artifact.state {
            ArtifactState::Pending => {
                self.artifacts
                    .upload(auth, artifact.id, artifact.revision, data.as_slice())
                    .await?;
            }
            ArtifactState::Ready => {
                let prior = self.artifacts.read(auth, artifact.id, 0..bytes).await?;
                if serde_json::from_slice::<serde_json::Value>(&prior)?
                    != serde_json::to_value(&result)?
                {
                    return Err(Error::Conflict);
                }
            }
            // Upload recovery is explicit: never reset a possibly live uploader.
            ArtifactState::Uploading | ArtifactState::Revoked => return Err(Error::Unavailable),
        }
        self.store
            .settle_usage(auth, fence, reservation.id, Some(usage))
            .await?;
        self.store
            .complete_job(auth, request_id, fence, result)
            .await
    }
}
