use crate::Host;
use memory_domain::{
    contracts::{Authority, Origin},
    coordination::{Fence, Resources},
    sources::{Artifact, ArtifactSpec, ArtifactState},
};
use memory_store::{Error, Result};
use uuid::Uuid;

impl Host {
    pub(crate) async fn publish_work_artifact(
        &self,
        auth: &Authority,
        fence: &Fence,
        id: Uuid,
        label: String,
        text: String,
        dependencies: Vec<Uuid>,
    ) -> Result<Artifact> {
        let job = self.store.assigned_job(auth, fence).await?;
        if job.cancel_requested
            || text.len() > 65536
            || label.trim().is_empty()
            || label.len() > 200
        {
            return Err(Error::LimitExceeded);
        }
        serde_json::from_str::<serde_json::Value>(&text)
            .map_err(|_| Error::Invalid("Work artifacts must contain JSON".into()))?;
        for dependency in &dependencies {
            self.store.input_artifact(auth, fence, *dependency).await?;
        }
        let usage = Resources {
            output_bytes: text.len() as u32,
            ..Default::default()
        };
        let reservation = self
            .store
            .reserve_usage(auth, fence, &format!("artifact:{id}"), usage, false)
            .await?;
        let artifact = self
            .artifacts
            .allocate_named(
                auth,
                id,
                ArtifactSpec {
                    label: format!("Job {}: {}", job.id, label),
                    scope: job.spec.brief.scope,
                    media_type: "application/json".into(),
                    expected_bytes: text.len() as u32,
                    origin: Origin::AgentGenerated,
                    retention_class: job.spec.brief.retention_policy,
                    dependencies,
                },
            )
            .await?;
        let result = match artifact.state {
            ArtifactState::Pending => {
                self.artifacts
                    .upload(auth, id, artifact.revision, text.as_bytes())
                    .await?
            }
            ArtifactState::Ready => {
                let prior = self.artifacts.read(auth, id, 0..text.len() as u32).await?;
                if prior.as_ref() != text.as_bytes() {
                    return Err(Error::Conflict);
                }
                artifact
            }
            _ => return Err(Error::Unavailable),
        };
        self.store.link_job_artifact(auth, fence, id).await?;
        self.store
            .settle_usage(auth, fence, reservation.id, Some(usage))
            .await?;
        Ok(result)
    }
}
