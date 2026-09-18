use crate::{Error, Result, Store, artifacts::ready_artifact, coordinator::fence};
use memory_domain::{contracts::Authority, coordination::Fence, judgement::*};
use sqlx::Row;
use uuid::Uuid;

impl Store {
    pub async fn check_judgement_packet(
        &self,
        auth: &Authority,
        permit: &Fence,
        packet: &JudgementPacket,
    ) -> Result<()> {
        packet.validate().map_err(Error::Invalid)?;
        let (mut tx, position) = self.begin_write().await?;
        let job = fence(&mut tx, auth, permit, position.recorded_at).await?;
        let brief = &job.spec.brief;
        if job.cancel_requested
            || packet.job_id != job.id
            || !brief.scope.permits(&packet.scope)
            || packet.policy != brief.policy
            || packet.budget_id != brief.limits.root_budget_id
            || packet.disclosure_policy != brief.disclosure_policy
            || packet.evidence_cutoff > brief.evidence_cutoff
            || packet.deadline > job.deadline
            || packet.deadline <= position.recorded_at
            || packet.expires_at > job.spec.retain_until
        {
            return Err(Error::Forbidden);
        }
        for r in &packet.inputs.sources {
            if !brief.capabilities.sources.contains(r) {
                return Err(Error::Forbidden);
            }
            crate::sources::source_version(&mut tx, auth, r).await?;
        }
        for r in &packet.inputs.memories {
            if !brief.inputs.memories.contains(r) {
                return Err(Error::Forbidden);
            }
            crate::memories::memory_version(&mut tx, auth, r).await?;
        }
        drop(tx);
        for id in &packet.inputs.artifacts {
            self.input_artifact(auth, permit, *id).await?;
        }
        if let Some(id) = packet.local_check_id {
            let check = self.task_local_check(auth, permit, id).await?;
            if check.expires_at < packet.deadline
                || !check.scope.permits(&packet.scope)
                || packet.questions.len() != 1
                || serde_json::to_value(&packet.questions[0].definition)?
                    != serde_json::to_value(&check.definition)?
                || packet
                    .allowed_providers
                    .iter()
                    .any(|p| !check.allowed_providers.contains(p))
            {
                return Err(Error::Forbidden);
            }
        }
        Ok(())
    }
    pub async fn record_assessment(
        &self,
        auth: &Authority,
        permit: &Fence,
        packet: &JudgementPacket,
        assessment: SemanticAssessment,
    ) -> Result<SemanticAssessment> {
        self.check_judgement_packet(auth, permit, packet).await?;
        assessment.validate(packet).map_err(Error::Invalid)?;
        let (mut tx, position) = self.begin_write().await?;
        let job = fence(&mut tx, auth, permit, position.recorded_at).await?;
        ready_artifact(&mut tx, auth, packet.id).await?;
        ready_artifact(&mut tx, auth, assessment.raw_artifact_id).await?;
        if assessment.assessed_at > position.recorded_at
            || !packet.allowed_providers.contains(&assessment.provider)
        {
            return Err(Error::Forbidden);
        }
        let reservation =
            sqlx::query("SELECT job_id FROM budget_reservations WHERE tenant_id=$1 AND id=$2")
                .bind(auth.tenant_id.to_string())
                .bind(assessment.reservation_id.to_string())
                .fetch_optional(&mut *tx)
                .await?;
        if reservation.is_none_or(|r| r.get::<String, _>("job_id") != job.id.to_string()) {
            return Err(Error::Forbidden);
        }
        let data = serde_json::to_string(&assessment)?;
        let prior =
            sqlx::query("SELECT data FROM semantic_assessments WHERE tenant_id=$1 AND id=$2")
                .bind(auth.tenant_id.to_string())
                .bind(assessment.id.to_string())
                .fetch_optional(&mut *tx)
                .await?;
        if let Some(prior) = prior {
            if serde_json::from_str::<serde_json::Value>(prior.get("data"))?
                != serde_json::to_value(&assessment)?
            {
                return Err(Error::Conflict);
            }
        } else {
            crate::access::write_scope(&mut tx, auth, assessment.id, &packet.scope).await?;
            sqlx::query("INSERT INTO semantic_assessments(tenant_id,id,job_id,packet_id,expires_at,data) VALUES($1,$2,$3,$4,$5,$6)").bind(auth.tenant_id.to_string()).bind(assessment.id.to_string()).bind(job.id.to_string()).bind(packet.id.to_string()).bind(assessment.expires_at.timestamp_millis()).bind(data).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(assessment)
    }
    pub async fn assessment(&self, auth: &Authority, id: Uuid) -> Result<SemanticAssessment> {
        let (mut tx, position) = self.begin_write().await?;
        crate::access::read_scope(&mut tx, auth, id).await?;
        let row = sqlx::query(
            "SELECT data FROM semantic_assessments WHERE tenant_id=$1 AND id=$2 AND expires_at>$3",
        )
        .bind(auth.tenant_id.to_string())
        .bind(id.to_string())
        .bind(position.recorded_at.timestamp_millis())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::Unavailable)?;
        let value: SemanticAssessment = serde_json::from_str(row.get("data"))?;
        ready_artifact(&mut tx, auth, value.packet_id).await?;
        ready_artifact(&mut tx, auth, value.raw_artifact_id).await?;
        Ok(value)
    }
    pub async fn record_judgement_decision(
        &self,
        auth: &Authority,
        permit: &Fence,
        packet: &JudgementPacket,
        decision: JudgementDecision,
    ) -> Result<JudgementDecision> {
        self.check_judgement_packet(auth, permit, packet).await?;
        if decision.decisions.len() != packet.questions.len()
            || decision.packet_id != packet.id
            || decision
                .selected_assessment
                .is_some_and(|id| !decision.assessment_ids.contains(&id))
            || decision
                .decisions
                .keys()
                .any(|id| !packet.questions.iter().any(|q| &q.definition.id == id))
            || decision.decisions.values().any(|d| {
                d.policy != packet.policy
                    || d.reason.trim().is_empty()
                    || d.expires_at.is_none_or(|at| at > packet.expires_at)
            })
        {
            return Err(Error::Invalid(
                "Decision must identify its policy and assessed questions".into(),
            ));
        }
        if packet.local_check_id.is_some()
            && decision.decisions.values().any(|d| {
                !matches!(
                    d.action,
                    memory_domain::records::PolicyAction::Investigate
                        | memory_domain::records::PolicyAction::RetrieveFurther
                        | memory_domain::records::PolicyAction::Defer
                )
            })
        {
            return Err(Error::Forbidden);
        }
        for id in &decision.assessment_ids {
            let assessment = self.assessment(auth, *id).await?;
            if !packet.allowed_providers.contains(&assessment.provider)
                || assessment.assessed_at < packet.fresh_after
                || (decision.selected_assessment == Some(*id)
                    && assessment.status != AssessmentStatus::Answered)
                || (assessment.packet_id != packet.id && assessment.model_release.is_none())
            {
                return Err(Error::Forbidden);
            }
        }
        let (mut tx, position) = self.begin_write().await?;
        let job = fence(&mut tx, auth, permit, position.recorded_at).await?;
        let prior =
            sqlx::query("SELECT data FROM judgement_decisions WHERE tenant_id=$1 AND id=$2")
                .bind(auth.tenant_id.to_string())
                .bind(decision.id.to_string())
                .fetch_optional(&mut *tx)
                .await?;
        if let Some(prior) = prior {
            if serde_json::from_str::<serde_json::Value>(prior.get("data"))?
                != serde_json::to_value(&decision)?
            {
                return Err(Error::Conflict);
            }
        } else {
            sqlx::query(
                "INSERT INTO judgement_decisions(tenant_id,id,job_id,data) VALUES($1,$2,$3,$4)",
            )
            .bind(auth.tenant_id.to_string())
            .bind(decision.id.to_string())
            .bind(job.id.to_string())
            .bind(serde_json::to_string(&decision)?)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(decision)
    }
    pub async fn register_task_check(
        &self,
        auth: &Authority,
        permit: &Fence,
        check: TaskLocalCheck,
    ) -> Result<TaskLocalCheck> {
        check.definition.validate().map_err(Error::Invalid)?;
        let (mut tx, position) = self.begin_write().await?;
        let job = fence(&mut tx, auth, permit, position.recorded_at).await?;
        if job.cancel_requested
            || check.job_id != job.id
            || check.task_id != job.spec.brief.task_id
            || !job.spec.brief.scope.permits(&check.scope)
            || check.disclosure_policy != job.spec.brief.disclosure_policy
            || check.expires_at > job.deadline
            || check.expires_at <= position.recorded_at
            || check.retired
            || check.definition.permitted_uses != ["investigation"]
            || check.completion_requirements != job.spec.brief.output_criteria
        {
            return Err(Error::Forbidden);
        }
        let prior = sqlx::query("SELECT data FROM task_local_checks WHERE tenant_id=$1 AND id=$2")
            .bind(auth.tenant_id.to_string())
            .bind(check.id.to_string())
            .fetch_optional(&mut *tx)
            .await?;
        if let Some(prior) = prior {
            if serde_json::from_str::<serde_json::Value>(prior.get("data"))?
                != serde_json::to_value(&check)?
            {
                return Err(Error::Conflict);
            }
        } else {
            sqlx::query(
                "INSERT INTO task_local_checks(tenant_id,id,job_id,data) VALUES($1,$2,$3,$4)",
            )
            .bind(auth.tenant_id.to_string())
            .bind(check.id.to_string())
            .bind(job.id.to_string())
            .bind(serde_json::to_string(&check)?)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(check)
    }
    pub async fn task_local_check(
        &self,
        auth: &Authority,
        permit: &Fence,
        id: Uuid,
    ) -> Result<TaskLocalCheck> {
        let (mut tx, position) = self.begin_write().await?;
        let job = fence(&mut tx, auth, permit, position.recorded_at).await?;
        let row = sqlx::query(
            "SELECT data FROM task_local_checks WHERE tenant_id=$1 AND id=$2 AND job_id=$3",
        )
        .bind(auth.tenant_id.to_string())
        .bind(id.to_string())
        .bind(job.id.to_string())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
        let check: TaskLocalCheck = serde_json::from_str(row.get("data"))?;
        if job.cancel_requested || check.retired || check.expires_at <= position.recorded_at {
            return Err(Error::Unavailable);
        }
        Ok(check)
    }
    pub async fn retire_task_check(
        &self,
        auth: &Authority,
        permit: &Fence,
        id: Uuid,
    ) -> Result<()> {
        let (mut tx, position) = self.begin_write().await?;
        let job = fence(&mut tx, auth, permit, position.recorded_at).await?;
        let row = sqlx::query(
            "SELECT data FROM task_local_checks WHERE tenant_id=$1 AND id=$2 AND job_id=$3",
        )
        .bind(auth.tenant_id.to_string())
        .bind(id.to_string())
        .bind(job.id.to_string())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
        let mut check: TaskLocalCheck = serde_json::from_str(row.get("data"))?;
        check.retired = true;
        sqlx::query("UPDATE task_local_checks SET data=$1 WHERE tenant_id=$2 AND id=$3")
            .bind(serde_json::to_string(&check)?)
            .bind(auth.tenant_id.to_string())
            .bind(id.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}
