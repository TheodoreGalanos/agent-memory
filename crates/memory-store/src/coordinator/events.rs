use super::{jobs::submit, receipts::existing};
use crate::{
    Error, Result, Store,
    access::{predicate, read_scope},
    database::{id, number, timestamp},
};
use memory_domain::{
    contracts::{Authority, Command, Scope},
    coordination::{Event, EventPage, Job, SubmitJob},
    records::CommitPosition,
};
use sqlx::{AnyConnection, Row};
use uuid::Uuid;

pub(crate) async fn append_event(
    connection: &mut AnyConnection,
    authority: &Authority,
    resource_id: Uuid,
    kind: &str,
    position: CommitPosition,
    expires: chrono::DateTime<chrono::Utc>,
) -> Result<()> {
    let cursor: i64 =
        sqlx::query("UPDATE commit_clock SET sequence=sequence+1 WHERE id=1 RETURNING sequence")
            .fetch_one(&mut *connection)
            .await?
            .try_get("sequence")?;
    sqlx::query("INSERT INTO outbox_events (tenant_id,id,cursor,resource_id,kind,recorded_at,expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7)").bind(authority.tenant_id.to_string()).bind(Uuid::now_v7().to_string()).bind(cursor).bind(resource_id.to_string()).bind(kind).bind(position.recorded_at.timestamp_millis()).bind(expires.timestamp_millis()).execute(connection).await?;
    Ok(())
}
impl Store {
    pub async fn events(&self, authority: &Authority, after: u32, limit: u16) -> Result<EventPage> {
        let mut connection = self.pool.acquire().await?;
        let ceiling = number(
            sqlx::query("SELECT sequence FROM commit_clock WHERE id=1")
                .fetch_one(&mut *connection)
                .await?
                .try_get("sequence")?,
        )?;
        if after > ceiling {
            return Err(Error::Invalid(
                "Event cursor is beyond the current position".into(),
            ));
        }
        let floor = sqlx::query("SELECT discarded_through FROM event_retention WHERE tenant_id=$1")
            .bind(authority.tenant_id.to_string())
            .fetch_optional(&mut *connection)
            .await?
            .map(|row| row.try_get::<i64, _>("discarded_through"))
            .transpose()?
            .unwrap_or(0);
        if i64::from(after) < floor {
            return Ok(EventPage {
                events: vec![],
                cursor: ceiling,
                snapshot_required: true,
            });
        }
        let (filter, mut values) = predicate(authority);
        values.push(after.to_string());
        let n = values.len();
        values.push(ceiling.to_string());
        let sql = format!(
            "SELECT e.* FROM outbox_events e JOIN resource_scopes s ON s.tenant_id=e.tenant_id AND s.id=e.resource_id WHERE {filter} AND e.cursor>CAST(${n} AS BIGINT) AND e.cursor<=CAST(${} AS BIGINT) ORDER BY e.cursor LIMIT {}",
            values.len(),
            limit.clamp(1, 1000)
        );
        let mut query = sqlx::query(&sql);
        for value in values {
            query = query.bind(value);
        }
        let rows = query.fetch_all(&mut *connection).await?;
        let events: Vec<Event> = rows
            .iter()
            .map(|row| {
                Ok(Event {
                    cursor: number(row.try_get("cursor")?)?,
                    id: id(row.try_get("id")?)?,
                    resource_id: id(row.try_get("resource_id")?)?,
                    kind: row.try_get("kind")?,
                    recorded_at: timestamp(row.try_get("recorded_at")?)?,
                })
            })
            .collect::<Result<_>>()?;
        let cursor = if events.len() == usize::from(limit.clamp(1, 1000)) {
            events.last().unwrap().cursor
        } else {
            ceiling
        };
        Ok(EventPage {
            events,
            cursor,
            snapshot_required: false,
        })
    }
    pub async fn consume_event(
        &self,
        authority: &Authority,
        consumer: &str,
        event_id: Uuid,
        command: Command<SubmitJob>,
    ) -> Result<Job> {
        if consumer.trim().is_empty() {
            return Err(Error::Invalid("Consumer identity is required".into()));
        }
        let (mut tx, position) = self.begin_write().await?;
        super::receipts::check_identity(authority, &command)?;
        let event =
            sqlx::query("SELECT resource_id FROM outbox_events WHERE tenant_id=$1 AND id=$2")
                .bind(authority.tenant_id.to_string())
                .bind(event_id.to_string())
                .fetch_optional(&mut *tx)
                .await?
                .ok_or(Error::NotFound)?;
        read_scope(&mut tx, authority, id(event.try_get("resource_id")?)?).await?;
        let prior = sqlx::query("SELECT job_id FROM inbox_receipts WHERE tenant_id=$1 AND consumer_id=$2 AND event_id=$3").bind(authority.tenant_id.to_string()).bind(consumer).bind(event_id.to_string()).fetch_optional(&mut *tx).await?;
        if let Some(prior) = prior {
            return super::job(&mut tx, authority, id(prior.try_get("job_id")?)?).await;
        }
        let result = if let Some(prior) = existing(
            &mut tx,
            authority,
            command.request_id,
            "submit_job",
            &command,
        )
        .await?
        {
            prior
        } else {
            submit(&mut tx, authority, &command, position, true).await?
        };
        sqlx::query("INSERT INTO inbox_receipts (tenant_id,consumer_id,event_id,job_id) VALUES ($1,$2,$3,$4)").bind(authority.tenant_id.to_string()).bind(consumer).bind(event_id.to_string()).bind(result.id.to_string()).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(result)
    }
    pub async fn prune_job_history(&self, authority: &Authority) -> Result<u64> {
        // This is tenant administration, never an operation assigned to a worker.
        if authority.scope != Scope::default() {
            return Err(Error::Forbidden);
        }
        let (mut tx, position) = self.begin_write().await?;
        let floor: i64 = sqlx::query("SELECT COALESCE(MAX(cursor),0) AS cutoff FROM outbox_events WHERE tenant_id=$1 AND expires_at<=$2").bind(authority.tenant_id.to_string()).bind(position.recorded_at.timestamp_millis()).fetch_one(&mut *tx).await?.try_get("cutoff")?;
        // Expiration can be non-monotonic. Discard only the contiguous expired prefix.
        let first_live: Option<i64> = sqlx::query(
            "SELECT MIN(cursor) AS cutoff FROM outbox_events WHERE tenant_id=$1 AND expires_at>$2",
        )
        .bind(authority.tenant_id.to_string())
        .bind(position.recorded_at.timestamp_millis())
        .fetch_one(&mut *tx)
        .await?
        .try_get("cutoff")?;
        let floor = floor.min(first_live.map(|v| v - 1).unwrap_or(floor));
        sqlx::query("DELETE FROM outbox_events WHERE tenant_id=$1 AND cursor<=$2")
            .bind(authority.tenant_id.to_string())
            .bind(floor)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO event_retention (tenant_id,discarded_through) VALUES ($1,$2) ON CONFLICT (tenant_id) DO UPDATE SET discarded_through=CASE WHEN excluded.discarded_through>event_retention.discarded_through THEN excluded.discarded_through ELSE event_retention.discarded_through END").bind(authority.tenant_id.to_string()).bind(floor).execute(&mut *tx).await?;
        let rows = sqlx::query("SELECT j.id FROM jobs j WHERE j.tenant_id=$1 AND j.retain_until<=$2 AND j.state IN ('completed','partial','failed','cancelled') AND NOT EXISTS (SELECT 1 FROM jobs c WHERE c.tenant_id=j.tenant_id AND c.root_id=j.id AND c.id<>j.id) AND NOT EXISTS (SELECT 1 FROM job_child_links l JOIN jobs p ON p.tenant_id=l.tenant_id AND p.id=l.parent_id WHERE l.tenant_id=j.tenant_id AND l.child_id=j.id AND p.state NOT IN ('completed','partial','failed','cancelled')) AND NOT EXISTS (SELECT 1 FROM effect_requests e WHERE e.tenant_id=j.tenant_id AND e.job_id=j.id AND e.state IN ('in_progress','outcome_unknown')) ORDER BY j.depth DESC LIMIT 1000").bind(authority.tenant_id.to_string()).bind(position.recorded_at.timestamp_millis()).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        let mut deleted = 0;
        for row in &rows {
            let job_id = crate::database::id(row.try_get("id")?)?;
            match self
                .begin_deletion(
                    authority,
                    memory_domain::retention::DeletionRequest {
                        id: uuid::Uuid::now_v7(),
                        resources: vec![job_id],
                    },
                )
                .await
            {
                Ok(_) => deleted += 1,
                Err(Error::NotFound | Error::Forbidden) => (),
                Err(error) => return Err(error),
            }
        }
        Ok(deleted)
    }
}
