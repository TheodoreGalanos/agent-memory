//! Authenticated host commands. The transport cannot supply its own authority.
mod activation;
mod commands;
mod consolidation;
mod decision;
mod formation;
pub mod http;
mod intentions;
mod judgements;
mod maintenance;
mod publication;
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
                | R::Experience { .. }
                | R::ScheduledWork { .. }
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
            | R::ProposeCandidates { fence, .. }
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
            | R::CheckDecision { fence, .. }
            | R::SpawnChild { fence, .. }
            | R::ChildJobs { fence, .. }
            | R::WaitChildren { fence, .. }
            | R::ReadInput { fence, .. }
            | R::PublishArtifact { fence, .. }
            | R::PublishExperience { fence, .. }
            | R::InspectAssignment { fence }
            | R::Start { fence, .. }
            | R::Renew { fence, .. }
            | R::Wait { fence, .. }
            | R::AcknowledgeCancellation { fence }
            | R::RecordValidation { fence, .. }
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
