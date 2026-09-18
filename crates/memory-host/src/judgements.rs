use crate::Host;
use memory_domain::{contracts::Authority, coordination::Fence, judgement::*};
use memory_store::{Error, Result};
use uuid::Uuid;

impl Host {
    pub(crate) async fn read_judgement_packet(
        &self,
        auth: &Authority,
        id: Uuid,
    ) -> Result<JudgementPacket> {
        let artifact = self.artifacts.inspect(auth, id).await?;
        if artifact.spec.expected_bytes > 65536 {
            return Err(Error::LimitExceeded);
        }
        let data = self
            .artifacts
            .read(auth, id, 0..artifact.spec.expected_bytes)
            .await?;
        Ok(serde_json::from_slice(&data)?)
    }
    pub(crate) async fn check_judgement(
        &self,
        auth: &Authority,
        fence: &Fence,
        id: Uuid,
        provider: &str,
    ) -> Result<JudgementPacket> {
        self.store.input_artifact(auth, fence, id).await?;
        let packet = self.read_judgement_packet(auth, id).await?;
        self.store
            .check_judgement_packet(auth, fence, &packet)
            .await?;
        self.store
            .check_execution(auth, fence, Some(provider), None)
            .await?;
        for question in &packet.questions {
            self.store
                .check_execution(auth, fence, None, Some(&question.definition.id))
                .await?;
        }
        if !packet.allowed_providers.iter().any(|p| p == provider)
            || (packet.disclosure_policy == "restricted"
                && !self.restricted_providers.iter().any(|p| p == provider))
        {
            return Err(Error::Forbidden);
        }
        Ok(packet)
    }
    pub(crate) async fn admit_judgement(
        &self,
        auth: &Authority,
        fence: &Fence,
        packet: JudgementPacket,
    ) -> Result<JudgementPacket> {
        self.store
            .check_judgement_packet(auth, fence, &packet)
            .await?;
        for evidence in &packet.evidence {
            let artifact = self.artifacts.inspect(auth, evidence.artifact_id).await?;
            if artifact.spec.expected_bytes > 65536 || !packet.scope.permits(&artifact.spec.scope) {
                return Err(Error::Forbidden);
            }
            let data = self
                .artifacts
                .read(auth, evidence.artifact_id, 0..artifact.spec.expected_bytes)
                .await?;
            let value: serde_json::Value = serde_json::from_slice(&data)?;
            if value.pointer(&evidence.pointer) != Some(&evidence.content)
                || serde_json::to_value(artifact.spec.origin)?
                    != serde_json::to_value(evidence.origin)?
            {
                return Err(Error::Invalid(
                    "Packet evidence differs from the referenced observation".into(),
                ));
            }
        }
        self.publish_work_artifact(
            auth,
            fence,
            packet.id,
            "Judgement packet".into(),
            serde_json::to_string(&packet)?,
            packet.inputs.artifacts.clone(),
        )
        .await?;
        Ok(packet)
    }
    pub(crate) async fn reuse_assessment(
        &self,
        auth: &Authority,
        fence: &Fence,
        id: Uuid,
        packet_id: Uuid,
        provider: &str,
        release: &str,
    ) -> Result<Option<Box<SemanticAssessment>>> {
        let packet = self
            .check_judgement(auth, fence, packet_id, provider)
            .await?;
        let prior = match self.store.assessment(auth, id).await {
            Ok(v) => v,
            Err(Error::NotFound | Error::Unavailable) => return Ok(None),
            Err(e) => return Err(e),
        };
        if prior.status != AssessmentStatus::Answered
            || prior.provider != provider
            || release.is_empty()
            || prior.model_release.as_deref() != Some(release)
            || prior.assessed_at < packet.fresh_after
        {
            return Ok(None);
        }
        let old = self.read_judgement_packet(auth, prior.packet_id).await?;
        if !equivalent(&old, &packet)? {
            return Ok(None);
        }
        // The new packet has just rechecked all referenced versions and current permissions.
        Ok(Some(Box::new(prior)))
    }
}
pub(crate) fn equivalent(a: &JudgementPacket, b: &JudgementPacket) -> Result<bool> {
    fn basis(p: &JudgementPacket) -> serde_json::Value {
        serde_json::json!({"subject":p.subject,"frame":p.frame,"scope":p.scope,"inputs":p.inputs,"evidence":p.evidence,"missing":p.missing,"questions":p.questions,"disclosure_policy":p.disclosure_policy,"allowed_providers":p.allowed_providers,"evidence_cutoff":p.evidence_cutoff,"local_check_id":p.local_check_id})
    }
    Ok(basis(a) == basis(b))
}
