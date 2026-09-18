use super::fence;
use crate::{
    Error, Result, Store,
    access::{read_scope, write_scope},
};
use memory_domain::{
    contracts::Authority,
    coordination::{Budget, BudgetUsage, Fence, Reservation, Resources, UsageKnowledge},
};
use sqlx::{AnyConnection, Row};
use uuid::Uuid;

impl Store {
    pub async fn create_budget(&self, authority: &Authority, budget: Budget) -> Result<Budget> {
        let (mut tx, position) = self.begin_write().await?;
        if budget.deadline <= position.recorded_at
            || !budget.final_result_reserve.fits(budget.limit)
            || budget.pricing_revision.trim().is_empty()
        {
            return Err(Error::Invalid(
                "Budget needs a future deadline, valid final reserve and pricing revision".into(),
            ));
        }
        write_scope(&mut tx, authority, budget.id, &budget.scope).await?;
        sqlx::query("INSERT INTO budget_accounts (tenant_id,id,data) VALUES ($1,$2,$3)")
            .bind(authority.tenant_id.to_string())
            .bind(budget.id.to_string())
            .bind(serde_json::to_string(&budget)?)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(budget)
    }
    pub async fn budget_usage(&self, authority: &Authority, id: Uuid) -> Result<BudgetUsage> {
        usage(&mut *self.pool.acquire().await?, authority, id).await
    }

    pub async fn reserve_usage(
        &self,
        authority: &Authority,
        permit: &Fence,
        provider_attempt: &str,
        maximum: Resources,
        final_result: bool,
    ) -> Result<Reservation> {
        if provider_attempt.trim().is_empty() {
            return Err(Error::Invalid(
                "Provider attempt identity is required".into(),
            ));
        }
        let (mut tx, position) = self.begin_write().await?;
        let job = fence(&mut tx, authority, permit, position.recorded_at).await?;
        crate::interaction::execution_gate(&mut tx, authority, &job).await?;
        if job.cancel_requested || job.deadline <= position.recorded_at {
            return Err(Error::Conflict);
        }
        let budget_id = job.spec.brief.limits.root_budget_id;
        let prior = sqlx::query("SELECT data FROM budget_reservations WHERE tenant_id=$1 AND budget_id=$2 AND provider_attempt=$3").bind(authority.tenant_id.to_string()).bind(budget_id.to_string()).bind(provider_attempt).fetch_optional(&mut *tx).await?;
        if let Some(prior) = prior {
            let prior: Reservation = serde_json::from_str(prior.try_get("data")?)?;
            if prior.job_id != job.id
                || prior.maximum != maximum
                || prior.final_result != final_result
            {
                return Err(Error::Conflict);
            }
            return Ok(prior);
        }
        let per_job = usage_for(&mut tx, authority, budget_id, Some(job.id)).await?;
        let job_total = per_job
            .committed
            .checked_add(per_job.unresolved)
            .and_then(|v| v.checked_add(maximum))
            .ok_or(Error::LimitExceeded)?;
        if job_total.tokens > job.spec.brief.limits.max_tokens.get()
            || job_total.provider_calls > job.spec.brief.limits.max_provider_attempts.get()
            || job_total.output_bytes > job.spec.brief.limits.max_output_bytes.get()
        {
            return Err(Error::LimitExceeded);
        }
        let used = usage(&mut tx, authority, budget_id).await?;
        let total = used
            .committed
            .checked_add(used.unresolved)
            .and_then(|v| v.checked_add(maximum))
            .and_then(|v| {
                v.checked_add(if final_result && job.id == job.root_id {
                    Resources::default()
                } else {
                    used.budget.final_result_reserve
                })
            })
            .ok_or(Error::LimitExceeded)?;
        if !total.fits(used.budget.limit) || used.budget.deadline <= position.recorded_at {
            return Err(Error::LimitExceeded);
        }
        let reservation = Reservation {
            id: Uuid::now_v7(),
            job_id: job.id,
            budget_id,
            provider_attempt: provider_attempt.into(),
            maximum,
            usage: None,
            knowledge: UsageKnowledge::Reserved,
            final_result,
        };
        sqlx::query("INSERT INTO budget_reservations (tenant_id,id,job_id,budget_id,provider_attempt,data) VALUES ($1,$2,$3,$4,$5,$6)").bind(authority.tenant_id.to_string()).bind(reservation.id.to_string()).bind(job.id.to_string()).bind(budget_id.to_string()).bind(provider_attempt).bind(serde_json::to_string(&reservation)?).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(reservation)
    }
    pub async fn settle_usage(
        &self,
        authority: &Authority,
        permit: &Fence,
        reservation_id: Uuid,
        observed: Option<Resources>,
    ) -> Result<Reservation> {
        let (mut tx, position) = self.begin_write().await?;
        let job = fence(&mut tx, authority, permit, position.recorded_at).await?;
        let row = sqlx::query("SELECT data FROM budget_reservations WHERE tenant_id=$1 AND id=$2")
            .bind(authority.tenant_id.to_string())
            .bind(reservation_id.to_string())
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(Error::NotFound)?;
        let mut reservation: Reservation = serde_json::from_str(row.try_get("data")?)?;
        if reservation.job_id != job.id {
            return Err(Error::Forbidden);
        }
        let knowledge = if observed.is_some() {
            UsageKnowledge::Known
        } else {
            UsageKnowledge::Unknown
        };
        if reservation.knowledge == UsageKnowledge::Known {
            if reservation.usage != observed {
                return Err(Error::Conflict);
            }
            return Ok(reservation);
        }
        // Even an overrun is recorded as observed usage. It blocks new reservations
        // instead of hiding a provider charge that exceeded its estimate.
        reservation.usage = observed;
        reservation.knowledge = knowledge;
        sqlx::query("UPDATE budget_reservations SET data=$1 WHERE tenant_id=$2 AND id=$3")
            .bind(serde_json::to_string(&reservation)?)
            .bind(authority.tenant_id.to_string())
            .bind(reservation_id.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(reservation)
    }
}
pub(crate) async fn budget(
    connection: &mut AnyConnection,
    authority: &Authority,
    id: Uuid,
) -> Result<Budget> {
    read_scope(connection, authority, id).await?;
    let row = sqlx::query("SELECT data FROM budget_accounts WHERE tenant_id=$1 AND id=$2")
        .bind(authority.tenant_id.to_string())
        .bind(id.to_string())
        .fetch_optional(connection)
        .await?
        .ok_or(Error::NotFound)?;
    Ok(serde_json::from_str(row.try_get("data")?)?)
}
async fn usage(
    connection: &mut AnyConnection,
    authority: &Authority,
    id: Uuid,
) -> Result<BudgetUsage> {
    usage_for(connection, authority, id, None).await
}
async fn usage_for(
    connection: &mut AnyConnection,
    authority: &Authority,
    id: Uuid,
    job_id: Option<Uuid>,
) -> Result<BudgetUsage> {
    let budget = budget(connection, authority, id).await?;
    let sql = if job_id.is_some() {
        "SELECT data FROM budget_reservations WHERE tenant_id=$1 AND budget_id=$2 AND job_id=$3"
    } else {
        "SELECT data FROM budget_reservations WHERE tenant_id=$1 AND budget_id=$2"
    };
    let mut query = sqlx::query(sql)
        .bind(authority.tenant_id.to_string())
        .bind(id.to_string());
    if let Some(job_id) = job_id {
        query = query.bind(job_id.to_string());
    }
    let rows = query.fetch_all(&mut *connection).await?;
    let mut committed = Resources::default();
    let mut unresolved = Resources::default();
    for row in rows {
        let reservation: Reservation = serde_json::from_str(row.try_get("data")?)?;
        // A root-only sum prevents counting parent/child reports twice.
        if reservation.knowledge == UsageKnowledge::Known {
            committed = committed
                .checked_add(reservation.usage.ok_or(Error::Unavailable)?)
                .ok_or(Error::LimitExceeded)?;
        } else {
            unresolved = unresolved
                .checked_add(reservation.maximum)
                .ok_or(Error::LimitExceeded)?;
        }
    }
    Ok(BudgetUsage {
        budget,
        committed,
        unresolved,
    })
}

// An expired owner may have reached the provider before losing its acknowledgement.
pub(crate) async fn mark_usage_unknown(
    connection: &mut AnyConnection,
    authority: &Authority,
    job_id: Uuid,
) -> Result<()> {
    let rows = sqlx::query("SELECT data FROM budget_reservations WHERE tenant_id=$1 AND job_id=$2")
        .bind(authority.tenant_id.to_string())
        .bind(job_id.to_string())
        .fetch_all(&mut *connection)
        .await?;
    for row in rows {
        let mut reservation: Reservation = serde_json::from_str(row.try_get("data")?)?;
        if reservation.knowledge == UsageKnowledge::Reserved {
            reservation.knowledge = UsageKnowledge::Unknown;
            sqlx::query("UPDATE budget_reservations SET data=$1 WHERE tenant_id=$2 AND id=$3")
                .bind(serde_json::to_string(&reservation)?)
                .bind(authority.tenant_id.to_string())
                .bind(reservation.id.to_string())
                .execute(&mut *connection)
                .await?;
        }
    }
    Ok(())
}
