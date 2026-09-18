use super::{fence, job};
use crate::{
    Error, Result, Store,
    access::require_write_scope,
    database::{from_tag, id, number, tag},
};
use memory_domain::{
    contracts::Authority,
    coordination::{Effect, EffectRequest, EffectResolution, EffectState, Fence, ReplayClass},
};
use sqlx::{AnyConnection, Row, any::AnyRow};
use uuid::Uuid;

impl Store {
    pub async fn prepare_effect(
        &self,
        authority: &Authority,
        permit: &Fence,
        request: EffectRequest,
    ) -> Result<Effect> {
        let (mut tx, position) = self.begin_write().await?;
        let job = fence(&mut tx, authority, permit, position.recorded_at).await?;
        crate::interaction::execution_gate(&mut tx, authority, &job).await?;
        if job.cancel_requested
            || request.logical_operation_id
                != format!("{}/{}", job.operation_id, request.invocation_id)
            || request.kind.trim().is_empty()
            || request.invocation_id.trim().is_empty()
        {
            return Err(Error::Forbidden);
        }
        let prior = sqlx::query("SELECT * FROM effect_requests WHERE tenant_id=$1 AND logical_operation_id=$2 AND kind=$3").bind(authority.tenant_id.to_string()).bind(&request.logical_operation_id).bind(&request.kind).fetch_optional(&mut *tx).await?;
        if let Some(prior) = prior {
            let prior = decode(&prior)?;
            if prior.job_id != job.id
                || serde_json::to_value(&prior.request)? != serde_json::to_value(&request)?
            {
                return Err(Error::Conflict);
            }
            return Ok(prior);
        }
        let effect = Effect {
            id: Uuid::now_v7(),
            job_id: job.id,
            request,
            state: EffectState::Prepared,
            epoch: permit.epoch,
            receipt: None,
        };
        sqlx::query("INSERT INTO effect_requests (tenant_id,id,job_id,logical_operation_id,kind,state,epoch,request) VALUES ($1,$2,$3,$4,$5,'prepared',$6,$7)").bind(authority.tenant_id.to_string()).bind(effect.id.to_string()).bind(job.id.to_string()).bind(&effect.request.logical_operation_id).bind(&effect.request.kind).bind(i64::from(permit.epoch)).bind(serde_json::to_string(&effect.request)?).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(effect)
    }
    pub async fn begin_effect(
        &self,
        authority: &Authority,
        permit: &Fence,
        effect_id: Uuid,
    ) -> Result<Effect> {
        let (mut tx, position) = self.begin_write().await?;
        let job = fence(&mut tx, authority, permit, position.recorded_at).await?;
        crate::interaction::execution_gate(&mut tx, authority, &job).await?;
        if job.cancel_requested {
            return Err(Error::Conflict);
        }
        let mut effect = effect(&mut tx, authority, effect_id).await?;
        if effect.job_id != job.id
            || effect.state == EffectState::Succeeded
            || effect.state == EffectState::Failed
        {
            return Err(Error::Conflict);
        }
        if effect.state != EffectState::Prepared && effect.request.replay == ReplayClass::Reconcile
        {
            return Err(Error::Conflict);
        }
        effect.state = EffectState::InProgress;
        effect.epoch = permit.epoch;
        update(&mut tx, authority, &effect).await?;
        tx.commit().await?;
        Ok(effect)
    }
    pub async fn report_effect(
        &self,
        authority: &Authority,
        permit: &Fence,
        effect_id: Uuid,
        state: EffectState,
        receipt: serde_json::Value,
    ) -> Result<Effect> {
        if !matches!(
            state,
            EffectState::Succeeded | EffectState::Failed | EffectState::OutcomeUnknown
        ) {
            return Err(Error::Invalid(
                "Report a terminal or unknown effect outcome".into(),
            ));
        }
        let (mut tx, position) = self.begin_write().await?;
        let job = fence(&mut tx, authority, permit, position.recorded_at).await?;
        let mut effect = effect(&mut tx, authority, effect_id).await?;
        if effect.job_id != job.id || effect.epoch != permit.epoch {
            return Err(Error::Conflict);
        }
        if effect.state == state && effect.receipt.as_ref() == Some(&receipt) {
            return Ok(effect);
        }
        if effect.state != EffectState::InProgress {
            return Err(Error::Conflict);
        }
        effect.state = state;
        effect.receipt = Some(receipt);
        update(&mut tx, authority, &effect).await?;
        tx.commit().await?;
        Ok(effect)
    }
    pub async fn effect(&self, authority: &Authority, id: Uuid) -> Result<Effect> {
        effect(&mut *self.pool.acquire().await?, authority, id).await
    }

    /// Host reconciliation requires observed external evidence, including an explicit
    /// observation of non-performance before a non-idempotent action can be repeated.
    pub async fn reconcile_effect(
        &self,
        authority: &Authority,
        id: Uuid,
        resolution: EffectResolution,
        evidence: serde_json::Value,
    ) -> Result<Effect> {
        if evidence.is_null() || evidence == serde_json::json!({}) {
            return Err(Error::Invalid(
                "Reconciliation needs observed evidence".into(),
            ));
        }
        let (mut tx, _) = self.begin_write().await?;
        let mut effect = effect(&mut tx, authority, id).await?;
        require_write_scope(&mut tx, authority, effect.job_id).await?;
        if effect.state != EffectState::OutcomeUnknown {
            return Err(Error::Conflict);
        }
        effect.state = match resolution {
            EffectResolution::Succeeded => EffectState::Succeeded,
            EffectResolution::Failed => EffectState::Failed,
            EffectResolution::NotPerformed => EffectState::Prepared,
        };
        effect.receipt = Some(evidence);
        update(&mut tx, authority, &effect).await?;
        tx.commit().await?;
        Ok(effect)
    }
}
async fn effect(connection: &mut AnyConnection, authority: &Authority, id: Uuid) -> Result<Effect> {
    let row = sqlx::query("SELECT * FROM effect_requests WHERE tenant_id=$1 AND id=$2")
        .bind(authority.tenant_id.to_string())
        .bind(id.to_string())
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(Error::NotFound)?;
    let result = decode(&row)?;
    job(connection, authority, result.job_id).await?;
    Ok(result)
}
fn decode(row: &AnyRow) -> Result<Effect> {
    Ok(Effect {
        id: id(row.try_get("id")?)?,
        job_id: id(row.try_get("job_id")?)?,
        request: serde_json::from_str(row.try_get("request")?)?,
        state: from_tag(row.try_get("state")?)?,
        epoch: number(row.try_get("epoch")?)?,
        receipt: row
            .try_get::<Option<String>, _>("receipt")?
            .map(|v| serde_json::from_str(&v))
            .transpose()?,
    })
}
async fn update(
    connection: &mut AnyConnection,
    authority: &Authority,
    effect: &Effect,
) -> Result<()> {
    sqlx::query(
        "UPDATE effect_requests SET state=$1,epoch=$2,receipt=$3 WHERE tenant_id=$4 AND id=$5",
    )
    .bind(tag(&effect.state)?)
    .bind(i64::from(effect.epoch))
    .bind(
        effect
            .receipt
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?,
    )
    .bind(authority.tenant_id.to_string())
    .bind(effect.id.to_string())
    .execute(connection)
    .await?;
    Ok(())
}
