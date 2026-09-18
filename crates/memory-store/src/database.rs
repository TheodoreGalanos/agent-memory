use crate::{Error, Result};
use chrono::{DateTime, Utc};
use memory_domain::records::CommitPosition;
use sqlx::{Any, AnyConnection, AnyPool, Row, Transaction, any::AnyPoolOptions};

#[derive(Clone)]
pub struct Store {
    pub(crate) pool: AnyPool,
    pub(crate) postgres: bool,
}

impl Store {
    pub fn is_postgres(&self) -> bool {
        self.postgres
    }
    /// Writes a consistent copy of the SQLite database to `path` while the store stays open.
    pub async fn snapshot_sqlite(&self, path: &std::path::Path) -> Result<()> {
        if self.postgres {
            return Err(Error::Invalid(
                "SQLite snapshots do not apply to PostgreSQL; use base backup and WAL archiving"
                    .into(),
            ));
        }
        let target = path
            .to_str()
            .ok_or_else(|| Error::Invalid("Backup path must be UTF-8".into()))?
            .replace('\'', "''");
        sqlx::query(&format!("VACUUM INTO '{target}'"))
            .execute(&self.pool)
            .await?;
        Ok(())
    }
    /// Current commit clock and applied schema version.
    pub async fn backup_position(&self) -> Result<(u64, u64)> {
        let mut connection = self.pool.acquire().await?;
        let sequence: i64 = sqlx::query("SELECT sequence FROM commit_clock WHERE id=1")
            .fetch_one(&mut *connection)
            .await?
            .try_get("sequence")?;
        let version: i64 =
            sqlx::query("SELECT COALESCE(MAX(version), 0) AS version FROM schema_migrations")
                .fetch_one(&mut *connection)
                .await?
                .try_get("version")?;
        Ok((
            u64::try_from(sequence).unwrap_or(0),
            u64::try_from(version).unwrap_or(0),
        ))
    }
    /// Operational counts with no tenant identifiers, for metrics.
    pub async fn job_counts(&self) -> Result<Vec<(String, i64)>> {
        let rows = sqlx::query("SELECT state, COUNT(*) AS count FROM jobs GROUP BY state")
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| {
                Ok((
                    row.try_get::<String, _>("state")?,
                    row.try_get::<i64, _>("count")?,
                ))
            })
            .collect()
    }
    /// A trivial round trip; readiness reports its failure instead of panicking.
    pub async fn ping(&self) -> Result<()> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }
    pub async fn connect(url: &str) -> Result<Self> {
        sqlx::any::install_default_drivers();
        let postgres = url.starts_with("postgres:") || url.starts_with("postgresql:");
        if !postgres && !url.starts_with("sqlite:") {
            return Err(Error::Invalid("Expected a SQLite or PostgreSQL URL".into()));
        }
        let pool = AnyPoolOptions::new()
            .max_connections(if postgres { 5 } else { 1 })
            .after_connect(move |connection, _| {
                Box::pin(async move {
                    if !postgres {
                        sqlx::query("PRAGMA foreign_keys = ON")
                            .execute(&mut *connection)
                            .await?;
                        sqlx::query("PRAGMA busy_timeout = 5000")
                            .execute(&mut *connection)
                            .await?;
                        sqlx::query("PRAGMA journal_mode = WAL")
                            .execute(&mut *connection)
                            .await?;
                        sqlx::query("PRAGMA secure_delete = ON")
                            .execute(&mut *connection)
                            .await?;
                        sqlx::query("PRAGMA synchronous = FULL")
                            .execute(&mut *connection)
                            .await?;
                    }
                    Ok(())
                })
            })
            .connect(url)
            .await?;
        let store = Self { pool, postgres };
        store.migrate().await?;
        Ok(store)
    }

    async fn migrate(&self) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        if self.postgres {
            // Lock before the first DDL statement: IF NOT EXISTS alone does not
            // coordinate two PostgreSQL processes creating an empty schema.
            sqlx::query("SELECT CAST(1 AS BIGINT) FROM pg_advisory_xact_lock(7184202)")
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("CREATE TABLE IF NOT EXISTS schema_migrations (version BIGINT PRIMARY KEY)")
            .execute(&mut *tx)
            .await?;
        sqlx::query("CREATE TABLE IF NOT EXISTS commit_clock (id BIGINT PRIMARY KEY, sequence BIGINT NOT NULL)").execute(&mut *tx).await?;
        sqlx::query(
            "INSERT INTO commit_clock (id, sequence) VALUES (1, 0) ON CONFLICT (id) DO NOTHING",
        )
        .execute(&mut *tx)
        .await?;
        // Serialise migrations with ordinary writes, including another process opening the store.
        sqlx::query("UPDATE commit_clock SET sequence = sequence WHERE id = 1")
            .execute(&mut *tx)
            .await?;
        let current: i64 =
            sqlx::query("SELECT COALESCE(MAX(version), 0) AS version FROM schema_migrations")
                .fetch_one(&mut *tx)
                .await?
                .try_get("version")?;
        let migrations = [
            include_str!("../migrations/001_records.sql"),
            include_str!("../migrations/002_sources.sql"),
            include_str!("../migrations/003_coordinator.sql"),
            include_str!("../migrations/004_scoped_operations.sql"),
            include_str!("../migrations/005_judgements.sql"),
            include_str!("../migrations/006_formation.sql"),
            include_str!("../migrations/007_activation.sql"),
            include_str!("../migrations/008_consolidation.sql"),
            include_str!("../migrations/009_maintenance.sql"),
            include_str!("../migrations/010_retention.sql"),
            include_str!("../migrations/011_interaction.sql"),
        ];
        if current > migrations.len() as i64 {
            return Err(Error::Invalid(
                "Database schema is newer than this worker".into(),
            ));
        }
        for (index, migration) in migrations.iter().enumerate().skip(current as usize) {
            for statement in migration.split(';').filter(|part| !part.trim().is_empty()) {
                sqlx::query(statement).execute(&mut *tx).await?;
            }
            sqlx::query("INSERT INTO schema_migrations (version) VALUES ($1)")
                .bind((index + 1) as i64)
                .execute(&mut *tx)
                .await?;
        }
        if current < 7 {
            let ddl = if self.postgres {
                "CREATE TABLE memory_search (version_id TEXT PRIMARY KEY, document TEXT NOT NULL)"
            } else {
                "CREATE VIRTUAL TABLE memory_search USING fts5(version_id UNINDEXED, document)"
            };
            sqlx::query(ddl).execute(&mut *tx).await?;
            // Existing versions are backfilled in this migration, including historical reads.
            let rows =
                sqlx::query("SELECT version_id,data,tenant_id,recorded_from FROM memory_versions")
                    .fetch_all(&mut *tx)
                    .await?;
            for row in rows {
                let record: memory_domain::records::RecordDraft =
                    serde_json::from_str(row.try_get("data")?)?;
                crate::activation::index_record(
                    &mut tx,
                    row.try_get("tenant_id")?,
                    row.try_get("version_id")?,
                    row.try_get("recorded_from")?,
                    &record,
                )
                .await?;
            }
            if self.postgres {
                sqlx::query("CREATE INDEX memory_search_text ON memory_search USING GIN (to_tsvector('simple', document))").execute(&mut *tx).await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    pub async fn position(&self) -> Result<u32> {
        let row = sqlx::query("SELECT sequence FROM commit_clock WHERE id = 1")
            .fetch_one(&self.pool)
            .await?;
        number(row.try_get("sequence")?)
    }

    pub(crate) async fn begin_write(&self) -> Result<(Transaction<'_, Any>, CommitPosition)> {
        let mut tx = self.pool.begin().await?;
        let sequence: i64 = sqlx::query(
            "UPDATE commit_clock SET sequence = sequence + 1 WHERE id = 1 RETURNING sequence",
        )
        .fetch_one(&mut *tx)
        .await?
        .try_get("sequence")?;
        let clock_sql = if self.postgres {
            "SELECT CAST(EXTRACT(EPOCH FROM clock_timestamp()) * 1000 AS BIGINT) AS time"
        } else {
            "SELECT CAST(ROUND((julianday('now') - 2440587.5) * 86400000) AS BIGINT) AS time"
        };
        let millis: i64 = sqlx::query(clock_sql)
            .fetch_one(&mut *tx)
            .await?
            .try_get("time")?;
        Ok((
            tx,
            CommitPosition {
                sequence: number(sequence)?,
                recorded_at: timestamp(millis)?,
            },
        ))
    }
}

pub(crate) fn number(value: i64) -> Result<u32> {
    value
        .try_into()
        .map_err(|_| Error::Invalid("Stored integer exceeds the wire range".into()))
}
pub(crate) fn timestamp(value: i64) -> Result<DateTime<Utc>> {
    DateTime::from_timestamp_millis(value)
        .ok_or_else(|| Error::Invalid("Stored timestamp is out of range".into()))
}
pub(crate) fn id(value: &str) -> Result<uuid::Uuid> {
    value
        .parse()
        .map_err(|_| Error::Invalid("Stored UUID is invalid".into()))
}
pub(crate) fn tag<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_value(value)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::Invalid("Expected an enum tag".into()))
}
pub(crate) fn from_tag<T: serde::de::DeserializeOwned>(value: String) -> Result<T> {
    Ok(serde_json::from_value(serde_json::Value::String(value))?)
}
pub(crate) fn changed(rows: u64) -> Result<()> {
    if rows == 1 {
        Ok(())
    } else {
        Err(Error::Conflict)
    }
}

pub(crate) async fn current_revision(
    connection: &mut AnyConnection,
    table: &str,
    tenant: &str,
    id: &str,
) -> Result<u32> {
    // Table names come only from the repository methods, never from API input.
    let row = sqlx::query(&format!(
        "SELECT revision FROM {table} WHERE tenant_id = $1 AND id = $2"
    ))
    .bind(tenant)
    .bind(id)
    .fetch_optional(connection)
    .await?
    .ok_or(Error::NotFound)?;
    number(row.try_get("revision")?)
}
