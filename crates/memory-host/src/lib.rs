//! Authenticated host commands. The transport cannot supply its own authority.
mod activation;
mod consolidation;
mod formation;
pub mod http;
mod intentions;
mod judgements;
mod maintenance;
mod qualification;
mod scoped;
pub mod supervisor;
use chrono::{DateTime, Utc};
use memory_domain::{
    contracts::{Authority, ContractError, ReasonCode},
    coordination::{HostRequest, HostResponse},
};
use memory_store::{Error, Store, artifacts::ArtifactService};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// Issued worker credentials are held only in memory; a supervisor reissues after a restart.
const MAX_ISSUED_CREDENTIALS: usize = 10_000;

/// Source snapshots accepted over HTTP; formation windows read admitted snapshots of the same size.
const MAX_SOURCE_SNAPSHOT_BYTES: u32 = 1_048_576;

#[derive(Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Role {
    Client,
    Worker {
        job_id: Uuid,
    },
    Administrator,
    /// A worker pool claims work in its classes and receives job-bound credentials.
    Pool {
        classes: Vec<memory_domain::coordination::WorkClass>,
    },
}
#[derive(Clone)]
pub struct Credential {
    pub token: String,
    pub authority: Authority,
    pub role: Role,
    pub expires_at: DateTime<Utc>,
}
pub struct Operations {
    pub uptime_seconds: u64,
    pub issued_worker_credentials: usize,
    pub jobs: Vec<(String, i64)>,
}

#[derive(Clone)]
pub struct Host {
    store: Store,
    artifacts: ArtifactService,
    artifact_root: std::path::PathBuf,
    credentials: Arc<RwLock<HashMap<String, Credential>>>,
    restricted_providers: Vec<String>,
    started: std::time::Instant,
}
impl Host {
    pub async fn new(
        store: Store,
        credentials: Vec<Credential>,
        artifact_root: impl AsRef<std::path::Path>,
    ) -> Result<Self, ContractError> {
        let mut configured = HashMap::new();
        for credential in credentials {
            if credential.token.len() < 32
                || credential.token.contains(char::is_whitespace)
                || configured.contains_key(&credential.token)
            {
                return Err(error(
                    ReasonCode::InvalidPayload,
                    "Credentials need distinct opaque tokens of at least 32 characters",
                ));
            }
            configured.insert(credential.token.clone(), credential);
        }
        if configured.is_empty() {
            return Err(error(
                ReasonCode::InvalidPayload,
                "At least one credential is required",
            ));
        }
        let artifacts = ArtifactService::local(store.clone(), &artifact_root)
            .await
            .map_err(map_error)?;
        Ok(Self {
            artifacts,
            artifact_root: artifact_root.as_ref().to_path_buf(),
            store,
            credentials: Arc::new(RwLock::new(configured)),
            restricted_providers: vec![],
            started: std::time::Instant::now(),
        })
    }
    /// Operational view for metrics and readiness; carries no tenant identifiers.
    pub async fn operations(&self) -> Result<Operations, Error> {
        let issued = {
            let credentials = self
                .credentials
                .read()
                .map_err(|_| Error::Invalid("Credential store is unavailable".into()))?;
            let now = Utc::now();
            credentials
                .values()
                .filter(|c| matches!(c.role, Role::Worker { .. }) && c.expires_at > now)
                .count()
        };
        Ok(Operations {
            uptime_seconds: self.started.elapsed().as_secs(),
            issued_worker_credentials: issued,
            jobs: self.store.job_counts().await?,
        })
    }
    pub async fn ready(&self) -> Result<(), Error> {
        self.store.ping().await
    }
    /// Providers reviewed by the operator for zero retained request content.
    pub fn with_restricted_providers(mut self, providers: Vec<String>) -> Self {
        self.restricted_providers = providers;
        self
    }
    pub async fn handle(
        &self,
        authorization: Option<&str>,
        request: HostRequest,
    ) -> Result<HostResponse, ContractError> {
        let credential = self.credential(authorization)?;
        authorize(&credential.role, &request)?;
        let exposure_request = serde_json::to_value(&request)
            .map_err(Error::from)
            .map_err(map_error)?;
        if let Role::Worker { job_id } = credential.role {
            self.store
                .record_exposure(&credential.authority, job_id, &serde_json::Value::Null)
                .await
                .map_err(map_error)?;
        }
        let response = self
            .execute(&credential.authority, request)
            .await
            .map_err(map_error)?;
        if let Role::Worker { job_id } = credential.role {
            self.store
                .record_exposure(
                    &credential.authority,
                    job_id,
                    &serde_json::json!({ "request": exposure_request, "response": response }),
                )
                .await
                .map_err(map_error)?;
        }
        Ok(response)
    }
    fn credential(&self, authorization: Option<&str>) -> Result<Credential, ContractError> {
        let token = authorization
            .and_then(|s| s.strip_prefix("Bearer "))
            .ok_or_else(|| {
                error(
                    ReasonCode::Unauthenticated,
                    "Bearer authentication is required",
                )
            })?;
        let credential = self
            .credentials
            .read()
            .map_err(|_| error(ReasonCode::InternalError, "Credential store is unavailable"))?
            .get(token)
            .filter(|c| c.expires_at > Utc::now())
            .cloned()
            .ok_or_else(|| {
                error(
                    ReasonCode::Unauthenticated,
                    "Credential is invalid or expired",
                )
            })?;
        Ok(credential)
    }
    /// Mints a worker credential for one job. The authority is the job's brief scope within
    /// the administrator's tenant; expiry is the job deadline. Expired issued tokens are pruned.
    async fn issue_worker_credential(
        &self,
        auth: &Authority,
        job_id: Uuid,
    ) -> Result<memory_domain::coordination::IssuedWorkerCredential, Error> {
        let job = self.store.job(auth, job_id).await?;
        self.mint_worker_credential(auth, &job)
    }
    fn mint_worker_credential(
        &self,
        auth: &Authority,
        job: &memory_domain::coordination::Job,
    ) -> Result<memory_domain::coordination::IssuedWorkerCredential, Error> {
        use rand::RngCore;
        if job.deadline <= Utc::now() {
            return Err(Error::Invalid("Job deadline has passed".into()));
        }
        let job_id = job.id;
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let issued = memory_domain::coordination::IssuedWorkerCredential {
            token: token.clone(),
            job_id,
            actor_id: Uuid::now_v7(),
            scope: job.spec.brief.scope.clone(),
            expires_at: job.deadline,
        };
        let credential = Credential {
            token,
            authority: Authority {
                tenant_id: auth.tenant_id,
                actor_id: issued.actor_id,
                scope: issued.scope.clone(),
            },
            role: Role::Worker { job_id },
            expires_at: issued.expires_at,
        };
        let mut credentials = self
            .credentials
            .write()
            .map_err(|_| Error::Invalid("Credential store is unavailable".into()))?;
        let now = Utc::now();
        credentials.retain(|_, c| c.expires_at > now);
        if credentials.len() >= MAX_ISSUED_CREDENTIALS {
            return Err(Error::LimitExceeded);
        }
        credentials.insert(credential.token.clone(), credential);
        Ok(issued)
    }
    /// Claims the oldest eligible job for the pool. The credential is minted first so the
    /// lease owner is the worker actor that will drive the job; a lost race moves on.
    async fn claim_next(
        &self,
        auth: &Authority,
        processes: &[memory_domain::contracts::Process],
        lease_seconds: u16,
    ) -> Result<HostResponse, Error> {
        for id in self.store.queued_jobs(auth, processes, 8).await? {
            let job = self.store.job(auth, id).await?;
            let issued = self.mint_worker_credential(auth, &job)?;
            let worker = Authority {
                tenant_id: auth.tenant_id,
                actor_id: issued.actor_id,
                scope: issued.scope.clone(),
            };
            match self.store.claim_job(&worker, id, lease_seconds).await {
                Ok(assignment) => {
                    return Ok(HostResponse::Work {
                        assignment: Box::new(assignment),
                        credential: Box::new(issued),
                    });
                }
                Err(Error::Conflict | Error::LimitExceeded) => {
                    self.forget_credential(&issued.token);
                }
                Err(other) => return Err(other),
            }
        }
        Ok(HostResponse::Idle)
    }
    fn forget_credential(&self, token: &str) {
        if let Ok(mut credentials) = self.credentials.write() {
            credentials.remove(token);
        }
    }
    /// Writes a consistent local backup: SQLite snapshot, artifact copy, deletion registry and manifest.
    async fn backup(
        &self,
        auth: &Authority,
        directory: std::path::PathBuf,
    ) -> Result<memory_domain::coordination::BackupManifest, Error> {
        use memory_domain::coordination::{BackupArtifacts, BackupDatabase, BackupManifest};
        use sha2::Digest;
        if directory.exists() {
            return Err(Error::Invalid("Backup directory must not exist".into()));
        }
        tokio::fs::create_dir_all(&directory).await?;
        let database_file = "memory.db";
        self.store
            .snapshot_sqlite(&directory.join(database_file))
            .await?;
        let bytes = tokio::fs::read(directory.join(database_file)).await?;
        let sha256 = hex::encode(sha2::Sha256::digest(&bytes));
        let (recovery_position, schema_version) = self.store.backup_position().await?;
        // Artifact objects are write-once; a directory copy of ready objects is consistent enough.
        let artifacts = copy_tree(&self.artifact_root, &directory.join("artifacts")).await?;
        let mut deletions = Vec::new();
        let mut after = None;
        loop {
            let page = self.store.deletions(auth, after, 100).await?;
            let Some(last) = page.last() else { break };
            after = Some(last.id);
            deletions.extend(page);
        }
        tokio::fs::write(
            directory.join("deletions.json"),
            serde_json::to_vec_pretty(&deletions)?,
        )
        .await?;
        let manifest = BackupManifest {
            created_at: Utc::now(),
            host_version: env!("CARGO_PKG_VERSION").into(),
            database: BackupDatabase {
                kind: "sqlite".into(),
                file: database_file.into(),
                bytes: bytes.len() as u64,
                sha256,
            },
            recovery_position,
            schema_version,
            artifacts: BackupArtifacts {
                root: "artifacts".into(),
                files: artifacts.0,
                bytes: artifacts.1,
            },
            deletions: u32::try_from(deletions.len()).unwrap_or(u32::MAX),
            tenants: vec![auth.tenant_id],
        };
        tokio::fs::write(
            directory.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )
        .await?;
        Ok(manifest)
    }
    /// Connectors publish source revisions through the adapters for each declared kind.
    async fn ingest_source(
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
    async fn publish_result(
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
    async fn execute(
        &self,
        auth: &Authority,
        request: HostRequest,
    ) -> memory_store::Result<HostResponse> {
        use HostRequest as R;
        use HostResponse as O;
        Ok(match request {
            R::User { request } => O::User {
                result: Box::new(self.store.user_request(auth, *request).await?),
            },
            R::RecordContext { fence, manifest } => {
                self.store.record_context(auth, &fence, *manifest).await?;
                O::Done { affected: 1 }
            }
            R::CheckProvider { fence, provider } => {
                self.store
                    .check_execution(auth, &fence, Some(&provider), None)
                    .await?;
                O::Done { affected: 1 }
            }
            R::ListDeletions { after_id, limit } => O::Deletions {
                reports: self.store.deletions(auth, after_id, limit).await?,
            },
            R::ReconcileDeletedEffect {
                deletion,
                effect_id,
                resolution,
                evidence_artifact,
            } => {
                self.store
                    .reconcile_deleted_effect(
                        auth,
                        deletion,
                        effect_id,
                        resolution,
                        evidence_artifact,
                    )
                    .await?;
                O::Deletion {
                    report: self.store.deletion_report(auth, deletion).await?,
                }
            }
            R::ContinueDeletedWork {
                deletion,
                old_job,
                command,
            } => O::Job {
                job: Box::new(
                    self.store
                        .continue_deleted_work(auth, deletion, old_job, *command)
                        .await?,
                ),
            },
            R::BeginDeletion { request } => O::Deletion {
                report: self.store.begin_deletion(auth, request).await?,
            },
            R::InspectDeletion { id } => O::Deletion {
                report: self.store.deletion_report(auth, id).await?,
            },
            R::PurgeDeletion { id } => O::Deletion {
                report: self.artifacts.purge_deletion(auth, id).await?,
            },
            R::AcknowledgePurge {
                id,
                boundary,
                container,
            } => O::Deletion {
                report: self
                    .store
                    .acknowledge_purge(auth, id, boundary, &container)
                    .await?,
            },
            R::RestoreDeletionRegistry { reports } => {
                self.store.restore_deletion_registry(auth, reports).await?;
                O::Done { affected: 0 }
            }
            R::AdmitJudgement { fence, packet } => O::JudgementPacket {
                packet: Box::new(self.admit_judgement(auth, &fence, *packet).await?),
            },
            R::CheckJudgement {
                fence,
                packet_id,
                provider,
            } => O::JudgementPacket {
                packet: Box::new(
                    self.check_judgement(auth, &fence, packet_id, &provider)
                        .await?,
                ),
            },
            R::RecordAssessment { fence, assessment } => {
                let packet = self
                    .check_judgement(auth, &fence, assessment.packet_id, &assessment.provider)
                    .await?;
                self.store
                    .input_artifact(auth, &fence, assessment.raw_artifact_id)
                    .await?;
                let raw = self
                    .artifacts
                    .inspect(auth, assessment.raw_artifact_id)
                    .await?;
                if !raw.spec.dependencies.contains(&packet.id) {
                    return Err(Error::Forbidden);
                }
                O::Assessment {
                    assessment: Some(Box::new(
                        self.store
                            .record_assessment(auth, &fence, &packet, *assessment)
                            .await?,
                    )),
                }
            }
            R::ReuseAssessment {
                fence,
                assessment_id,
                packet_id,
                provider,
                model_release,
            } => O::Assessment {
                assessment: self
                    .reuse_assessment(
                        auth,
                        &fence,
                        assessment_id,
                        packet_id,
                        &provider,
                        &model_release,
                    )
                    .await?,
            },
            R::RecordJudgementDecision { fence, decision } => {
                self.store
                    .input_artifact(auth, &fence, decision.packet_id)
                    .await?;
                let packet = self.read_judgement_packet(auth, decision.packet_id).await?;
                for id in &decision.assessment_ids {
                    let assessment = self.store.assessment(auth, *id).await?;
                    let basis = self
                        .read_judgement_packet(auth, assessment.packet_id)
                        .await?;
                    if !judgements::equivalent(&basis, &packet)? {
                        return Err(Error::Forbidden);
                    }
                }
                O::JudgementDecision {
                    decision: Box::new(
                        self.store
                            .record_judgement_decision(auth, &fence, &packet, *decision)
                            .await?,
                    ),
                }
            }
            R::RegisterTaskCheck { fence, check } => O::TaskCheck {
                check: Box::new(self.store.register_task_check(auth, &fence, *check).await?),
            },
            R::InspectTaskCheck { fence, id } => O::TaskCheck {
                check: Box::new(self.store.task_local_check(auth, &fence, id).await?),
            },
            R::RetireTaskCheck { fence, id } => {
                self.store.retire_task_check(auth, &fence, id).await?;
                O::Done { affected: 1 }
            }
            R::CreateBudget { budget } => O::Budget {
                budget: Box::new(self.store.create_budget(auth, *budget).await?),
            },
            R::Submit { command } => O::Job {
                job: Box::new(self.store.submit_job(auth, *command).await?),
            },
            R::SpawnChild {
                fence,
                request_id,
                brief,
                deadline,
                reuse_job_id,
            } => O::Job {
                job: Box::new(
                    self.store
                        .spawn_child(auth, &fence, request_id, *brief, deadline, reuse_job_id)
                        .await?,
                ),
            },
            R::ChildJobs { fence, ids } => O::Children {
                jobs: self.store.child_jobs(auth, &fence, &ids).await?,
            },
            R::WaitChildren {
                fence,
                ids,
                ready_at,
            } => {
                self.store
                    .wait_children(auth, &fence, &ids, ready_at)
                    .await?;
                O::Done { affected: 1 }
            }
            R::ReadInput {
                fence,
                artifact_id,
                offset,
                limit,
            } => {
                if limit == 0 || limit > 65536 {
                    return Err(Error::LimitExceeded);
                }
                self.store.input_artifact(auth, &fence, artifact_id).await?;
                let artifact = self.artifacts.inspect(auth, artifact_id).await?;
                let end = offset
                    .checked_add(limit)
                    .ok_or(Error::LimitExceeded)?
                    .min(artifact.spec.expected_bytes);
                let data = self.artifacts.read(auth, artifact_id, offset..end).await?;
                O::ArtifactData {
                    text: String::from_utf8(data.to_vec())
                        .map_err(|_| Error::Invalid("Text input must be UTF-8".into()))?,
                }
            }
            R::PublishArtifact {
                fence,
                request_id,
                label,
                text,
                dependencies,
            } => O::Artifact {
                artifact: Box::new(
                    self.publish_work_artifact(auth, &fence, request_id, label, text, dependencies)
                        .await?,
                ),
            },
            R::ConsolidationWindow {
                fence,
                id,
                selection,
            } => O::ConsolidationWindow {
                window: Box::new(
                    self.consolidation_window(auth, &fence, id, selection)
                        .await?,
                ),
            },
            R::ReviewConsolidation {
                fence,
                id,
                proposal,
            } => O::ConsolidationReview {
                review: Box::new(
                    self.review_consolidation(auth, &fence, id, *proposal)
                        .await?,
                ),
            },
            R::CommitConsolidation { fence, request } => O::ConsolidationResult {
                result: Box::new(self.commit_consolidation(auth, &fence, request).await?),
            },
            R::StartQualification {
                fence,
                request_id,
                review_id,
                suite_id,
            } => O::Job {
                job: Box::new(
                    self.start_qualification(auth, &fence, request_id, review_id, suite_id)
                        .await?,
                ),
            },
            R::AdoptProcedure {
                fence,
                evaluation_job,
            } => O::AdoptionResult {
                result: Box::new(self.adopt_procedure(auth, &fence, evaluation_job).await?),
            },
            R::EmbeddingInputs { after, limit } => O::EmbeddingInputs {
                inputs: self.store.embedding_inputs(auth, after, limit).await?,
            },
            R::Activate {
                fence,
                id,
                query,
                cursor,
            } => O::ActivationWindow {
                window: Box::new(self.activate(auth, &fence, id, *query, cursor).await?),
            },
            R::SelectActivation { fence, selection } => O::ContextPackage {
                package: Box::new(self.select_activation(auth, &fence, selection).await?),
            },
            R::SaveEmbedding {
                reference,
                embedding,
            } => {
                self.store
                    .save_embedding(auth, &reference, embedding)
                    .await?;
                O::Done { affected: 1 }
            }
            R::FormationWindow {
                fence,
                id,
                source,
                operation,
                limit,
            } => O::FormationWindow {
                window: Box::new(
                    self.capture_window(auth, &fence, id, source, operation, limit)
                        .await?,
                ),
            },
            R::CommitFormation { fence, request } => O::FormationResult {
                result: Box::new(self.form_memories(auth, &fence, *request).await?),
            },
            R::InspectJob { job_id } => O::Job {
                job: Box::new(self.store.job(auth, job_id).await?),
            },
            R::Backup { directory } => O::Backup {
                manifest: Box::new(self.backup(auth, directory).await?),
            },
            R::ClaimNext {
                processes,
                lease_seconds,
            } => self.claim_next(auth, &processes, lease_seconds).await?,
            R::IssueWorkerCredential { job_id } => O::WorkerCredential {
                credential: Box::new(self.issue_worker_credential(auth, job_id).await?),
            },
            R::IngestSource { source, content } => O::Source {
                source: Box::new(self.ingest_source(auth, source, content).await?),
            },
            R::Claim {
                job_id,
                lease_seconds,
            } => O::Assignment {
                assignment: Box::new(self.store.claim_job(auth, job_id, lease_seconds).await?),
            },
            R::InspectAssignment { fence } => O::Assignment {
                assignment: Box::new(self.store.inspect_assignment(auth, &fence).await?),
            },
            R::Start { fence } => O::Job {
                job: Box::new(self.store.start_job(auth, &fence).await?),
            },
            R::Renew {
                fence,
                lease_seconds,
            } => O::Assignment {
                assignment: Box::new(
                    self.store
                        .renew_assignment(auth, &fence, lease_seconds)
                        .await?,
                ),
            },
            R::Wait {
                fence,
                reason,
                ready_at,
            } => {
                self.store.wait_job(auth, &fence, &reason, ready_at).await?;
                O::Done { affected: 1 }
            }
            R::Cancel { job_id } => {
                self.store.cancel_job(auth, job_id).await?;
                O::Done { affected: 1 }
            }
            R::AcknowledgeCancellation { fence } => O::Job {
                job: Box::new(self.store.acknowledge_cancellation(auth, &fence).await?),
            },
            R::Complete {
                request_id,
                fence,
                result,
            } => O::Job {
                job: Box::new(
                    self.publish_result(auth, request_id, &fence, *result)
                        .await?,
                ),
            },
            R::CommitMemories { command } => O::Memories {
                memories: self.store.commit_memories(auth, *command).await?,
            },
            R::Reserve {
                fence,
                provider_attempt,
                maximum,
                final_result,
            } => O::Reservation {
                reservation: self
                    .store
                    .reserve_usage(auth, &fence, &provider_attempt, maximum, final_result)
                    .await?,
            },
            R::SettleUsage {
                fence,
                reservation_id,
                observed,
            } => O::Reservation {
                reservation: self
                    .store
                    .settle_usage(auth, &fence, reservation_id, observed)
                    .await?,
            },
            R::BudgetUsage { budget_id } => O::BudgetUsage {
                usage: Box::new(self.store.budget_usage(auth, budget_id).await?),
            },
            R::PrepareEffect { fence, request } => O::Effect {
                effect: self.store.prepare_effect(auth, &fence, request).await?,
            },
            R::BeginEffect { fence, effect_id } => O::Effect {
                effect: self.store.begin_effect(auth, &fence, effect_id).await?,
            },
            R::ReportEffect {
                fence,
                effect_id,
                state,
                receipt,
            } => O::Effect {
                effect: self
                    .store
                    .report_effect(auth, &fence, effect_id, state, receipt)
                    .await?,
            },
            R::InspectEffect { effect_id } => O::Effect {
                effect: self.store.effect(auth, effect_id).await?,
            },
            R::ReconcileEffect {
                effect_id,
                resolution,
                evidence,
            } => O::Effect {
                effect: self
                    .store
                    .reconcile_effect(auth, effect_id, resolution, evidence)
                    .await?,
            },
            R::Events { after, limit } => O::Events {
                page: self.store.events(auth, after, limit).await?,
            },
            R::ConsumeEvent {
                consumer,
                event_id,
                command,
            } => O::Job {
                job: Box::new(
                    self.store
                        .consume_event(auth, &consumer, event_id, *command)
                        .await?,
                ),
            },
            R::AllocateArtifact { id, spec } => O::Artifact {
                artifact: Box::new(self.artifacts.allocate_named(auth, id, *spec).await?),
            },
            R::RecoverArtifactUpload { id, revision } => O::Artifact {
                artifact: Box::new(self.artifacts.recover_upload(auth, id, revision).await?),
            },
            R::ReviewMaintenance { fence, id, request } => O::MaintenanceReview {
                review: Box::new(self.review_maintenance(auth, &fence, id, *request).await?),
            },
            R::CommitMaintenance { fence, request } => O::MaintenanceResult {
                result: Box::new(self.commit_maintenance(auth, &fence, request).await?),
            },
            R::MemoryChanges {
                fence,
                after,
                limit,
            } => O::Changes {
                page: self
                    .store
                    .memory_changes(&self.maintenance_scope(auth, &fence).await?, after, limit)
                    .await?,
            },
            R::CurrentMemories { fence, references } => O::Memories {
                memories: self.current_memories(auth, &fence, references).await?,
            },
            R::IntentionCheck {
                fence,
                id,
                occurrence_id,
                kind,
            } => O::IntentionCheck {
                check: Box::new(
                    self.intention_check(auth, &fence, id, occurrence_id, kind)
                        .await?,
                ),
            },
            R::ApplyIntentionCheck {
                fence,
                check_id,
                decisions,
            } => O::Intention {
                occurrence: Box::new(
                    self.apply_intention_check(auth, &fence, check_id, decisions)
                        .await?,
                ),
            },
            R::InspectIntentions { definition_id } => O::Intentions {
                occurrences: self.store.intentions(auth, definition_id).await?,
            },
            R::CancelIntention {
                occurrence_id,
                reason,
            } => O::Intention {
                occurrence: Box::new(
                    self.store
                        .cancel_intention(auth, occurrence_id, reason)
                        .await?,
                ),
            },
            R::ConfirmIntention { occurrence_id } => O::Intention {
                occurrence: Box::new(self.store.confirm_intention(auth, occurrence_id).await?),
            },
            R::SweepIntentions { limit } => O::IntentionSweep {
                result: self.store.sweep_intentions(auth, limit).await?,
            },
            R::Recover => O::Done {
                affected: u64::from(self.store.recover_jobs(auth).await?),
            },
            R::PruneHistory => O::Done {
                affected: self.store.prune_job_history(auth).await?,
            },
        })
    }
}
fn authorize(role: &Role, request: &HostRequest) -> Result<(), ContractError> {
    use HostRequest as R;
    let allowed = match role {
        Role::Administrator => true,
        Role::Client if matches!(request, R::User { .. }) => match request {
            R::User { request } => match request.as_ref() {
                memory_domain::interaction::UserRequest::Mutate { mutation, .. } => {
                    !mutation.administrator_only()
                }
                _ => true,
            },
            _ => false,
        },
        Role::Client => matches!(
            request,
            R::Submit { .. }
                | R::InspectJob { .. }
                | R::IngestSource { .. }
                | R::Cancel { .. }
                | R::Events { .. }
                | R::BudgetUsage { .. }
                | R::InspectIntentions { .. }
                | R::CancelIntention { .. }
                | R::ConfirmIntention { .. }
        ),
        Role::Pool { classes } => match request {
            R::ClaimNext { processes, .. } => {
                !processes.is_empty()
                    && processes
                        .iter()
                        .all(|p| classes.contains(&memory_domain::coordination::WorkClass::of(*p)))
            }
            _ => false,
        },
        Role::Worker { job_id } => match request {
            R::User { request } => {
                matches!(request.as_ref(), memory_domain::interaction::UserRequest::Mutate { mutation,.. }
                if matches!(mutation.as_ref(), memory_domain::interaction::UserMutation::RequestDecision { job_id: requested, fence:Some(fence),.. } if requested==job_id && fence.job_id==*job_id))
            }
            R::Claim {
                job_id: requested, ..
            }
            | R::InspectJob { job_id: requested } => requested == job_id,
            R::RecordContext { fence, .. }
            | R::CheckProvider { fence, .. }
            | R::ReviewMaintenance { fence, .. }
            | R::CommitMaintenance { fence, .. }
            | R::MemoryChanges { fence, .. }
            | R::CurrentMemories { fence, .. }
            | R::IntentionCheck { fence, .. }
            | R::ApplyIntentionCheck { fence, .. }
            | R::Activate { fence, .. }
            | R::SelectActivation { fence, .. }
            | R::FormationWindow { fence, .. }
            | R::CommitFormation { fence, .. }
            | R::AdmitJudgement { fence, .. }
            | R::CheckJudgement { fence, .. }
            | R::RecordAssessment { fence, .. }
            | R::ReuseAssessment { fence, .. }
            | R::RecordJudgementDecision { fence, .. }
            | R::ConsolidationWindow { fence, .. }
            | R::ReviewConsolidation { fence, .. }
            | R::CommitConsolidation { fence, .. }
            | R::StartQualification { fence, .. }
            | R::AdoptProcedure { fence, .. }
            | R::RegisterTaskCheck { fence, .. }
            | R::InspectTaskCheck { fence, .. }
            | R::RetireTaskCheck { fence, .. }
            | R::SpawnChild { fence, .. }
            | R::ChildJobs { fence, .. }
            | R::WaitChildren { fence, .. }
            | R::ReadInput { fence, .. }
            | R::PublishArtifact { fence, .. }
            | R::InspectAssignment { fence }
            | R::Start { fence }
            | R::Renew { fence, .. }
            | R::Wait { fence, .. }
            | R::AcknowledgeCancellation { fence }
            | R::Complete { fence, .. }
            | R::Reserve {
                fence,
                final_result: false,
                ..
            }
            | R::SettleUsage { fence, .. }
            | R::PrepareEffect { fence, .. }
            | R::BeginEffect { fence, .. }
            | R::ReportEffect { fence, .. } => fence.job_id == *job_id,
            R::CommitMemories { command } => {
                command.payload.fence.job_id == *job_id && command.job_id == Some(*job_id)
            }
            _ => false,
        },
    };
    if allowed {
        Ok(())
    } else {
        Err(error(
            ReasonCode::ForbiddenScope,
            "Operation exceeds this credential's role or assignment",
        ))
    }
}
fn error(code: ReasonCode, message: &str) -> ContractError {
    ContractError {
        code,
        message: message.into(),
    }
}
fn map_error(value: Error) -> ContractError {
    match value {
        Error::Forbidden => error(
            ReasonCode::ForbiddenScope,
            "Operation exceeds assigned scope",
        ),
        Error::NotFound | Error::Unavailable => error(
            ReasonCode::SourceUnavailable,
            "Resource is unavailable in this scope",
        ),
        Error::Conflict => error(
            ReasonCode::RequestConflict,
            "Revision, ownership or request conflicts with current state",
        ),
        Error::Invalid(message) => error(ReasonCode::InvalidPayload, &message),
        Error::LimitExceeded => error(
            ReasonCode::BudgetExhausted,
            "Operation exceeds its resource allowance",
        ),
        // Database paths, queries and credentials do not become API error text.
        _ => error(
            ReasonCode::InternalError,
            "The host could not complete this operation",
        ),
    }
}

/// Copies a directory tree; returns (files, bytes). A missing source yields an empty copy.
async fn copy_tree(from: &std::path::Path, to: &std::path::Path) -> Result<(u64, u64), Error> {
    tokio::fs::create_dir_all(to).await?;
    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut pending = vec![from.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(mut entries) = tokio::fs::read_dir(&directory).await else {
            continue;
        };
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let relative = path
                .strip_prefix(from)
                .map_err(|_| Error::Invalid("Artifact path escapes root".into()))?;
            if entry.file_type().await?.is_dir() {
                tokio::fs::create_dir_all(to.join(relative)).await?;
                pending.push(path);
            } else {
                bytes += tokio::fs::copy(&path, to.join(relative)).await?;
                files += 1;
            }
        }
    }
    Ok((files, bytes))
}
