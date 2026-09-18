//! ABOUTME: Local-to-production migration (§20.4): copies every table from a SQLite snapshot into an
//! ABOUTME: empty PostgreSQL store under unchanged identities, in foreign-key order, then verifies counts.
use crate::{Error, Result, Store};
use serde::Serialize;
use sqlx::{AnyPool, Row};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize)]
pub struct TableCopy {
    pub table: String,
    pub rows: u64,
}
#[derive(Debug, Clone, Serialize)]
pub struct MigrationReport {
    pub tables: Vec<TableCopy>,
    pub rows: u64,
    pub commit_sequence: i64,
}

/// Tables the target creates itself; their contents are not copied.
const OWNED_BY_TARGET: &[&str] = &["schema_migrations"];

/// The source must be a SQLite file (a consistent `backup` snapshot); the target an empty
/// PostgreSQL store at the same schema version. Identities and the commit clock are preserved.
pub async fn copy_sqlite_to_postgres(
    source_url: &str,
    target_url: &str,
) -> Result<MigrationReport> {
    let source = Store::connect(source_url).await?;
    let target = Store::connect(target_url).await?;
    if source.postgres || !target.postgres {
        return Err(Error::Invalid(
            "Migration copies a SQLite source into a PostgreSQL target".into(),
        ));
    }
    let (source_position, source_schema) = source.backup_position().await?;
    let (target_position, target_schema) = target.backup_position().await?;
    if source_schema != target_schema {
        return Err(Error::Invalid(format!(
            "Schema versions differ: source {source_schema}, target {target_schema}"
        )));
    }
    let scopes: i64 = sqlx::query("SELECT COUNT(*) AS count FROM resource_scopes")
        .fetch_one(&target.pool)
        .await?
        .try_get("count")?;
    if target_position != 0 || scopes != 0 {
        return Err(Error::Conflict);
    }
    let order = table_order(&source.pool).await?;
    let mut tx = target.pool.begin().await?;
    let mut report = MigrationReport {
        tables: Vec::new(),
        rows: 0,
        commit_sequence: 0,
    };
    for table in &order {
        // FTS5 shadow tables are SQLite internals; the search projection itself copies as rows.
        if OWNED_BY_TARGET.contains(&table.as_str()) || table.starts_with("memory_search_") {
            continue;
        }
        let columns = columns(&source.pool, table).await?;
        if columns.is_empty() {
            continue;
        }
        let names: Vec<String> = columns
            .iter()
            .map(|(name, _)| format!("\"{name}\""))
            .collect();
        let rows = sqlx::query(&format!("SELECT {} FROM \"{table}\"", names.join(",")))
            .fetch_all(&source.pool)
            .await?;
        let copied = rows.len() as u64;
        if table == "commit_clock" {
            // The target already has its clock row; carry the source position across.
            for row in &rows {
                let sequence: i64 = row.try_get("sequence")?;
                sqlx::query("UPDATE commit_clock SET sequence=$1 WHERE id=1")
                    .bind(sequence)
                    .execute(&mut *tx)
                    .await?;
                report.commit_sequence = sequence;
            }
        } else {
            let placeholders: Vec<String> = (1..=columns.len()).map(|i| format!("${i}")).collect();
            let insert = format!(
                "INSERT INTO \"{table}\" ({}) VALUES ({})",
                names.join(","),
                placeholders.join(",")
            );
            for row in &rows {
                let mut query = sqlx::query(&insert);
                for (index, (_, integer)) in columns.iter().enumerate() {
                    if *integer {
                        query = query.bind(row.try_get::<Option<i64>, _>(index)?);
                    } else {
                        query = query.bind(row.try_get::<Option<String>, _>(index)?);
                    }
                }
                query.execute(&mut *tx).await?;
            }
        }
        report.tables.push(TableCopy {
            table: table.clone(),
            rows: copied,
        });
        report.rows += copied;
    }
    tx.commit().await?;
    // Verify: every copied table has the same count on both sides and the clock matches.
    for copy in &report.tables {
        let count: i64 = sqlx::query(&format!("SELECT COUNT(*) AS count FROM \"{}\"", copy.table))
            .fetch_one(&target.pool)
            .await?
            .try_get("count")?;
        if u64::try_from(count).unwrap_or(0) != copy.rows {
            return Err(Error::Invalid(format!(
                "Row count mismatch in {}: source {}, target {count}",
                copy.table, copy.rows
            )));
        }
    }
    if u64::try_from(report.commit_sequence).unwrap_or(0) != source_position {
        return Err(Error::Invalid("Commit clock was not carried across".into()));
    }
    Ok(report)
}

/// SQLite tables in an order that satisfies their declared foreign keys.
async fn table_order(pool: &AnyPool) -> Result<Vec<String>> {
    let rows = sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .fetch_all(pool)
        .await?;
    let tables: Vec<String> = rows
        .iter()
        .map(|r| r.try_get::<String, _>("name"))
        .collect::<std::result::Result<_, _>>()?;
    let mut depends: HashMap<String, HashSet<String>> = HashMap::new();
    for table in &tables {
        let keys = sqlx::query(&format!("PRAGMA foreign_key_list(\"{table}\")"))
            .fetch_all(pool)
            .await?;
        let entry = depends.entry(table.clone()).or_default();
        for key in keys {
            let parent: String = key.try_get("table")?;
            if &parent != table {
                entry.insert(parent);
            }
        }
    }
    let mut ordered = Vec::new();
    let mut placed: HashSet<String> = HashSet::new();
    while ordered.len() < tables.len() {
        let mut progressed = false;
        for table in &tables {
            if placed.contains(table) {
                continue;
            }
            if depends[table]
                .iter()
                .all(|parent| placed.contains(parent) || !tables.contains(parent))
            {
                ordered.push(table.clone());
                placed.insert(table.clone());
                progressed = true;
            }
        }
        if !progressed {
            return Err(Error::Invalid(
                "Foreign keys form a cycle; migration order is undefined".into(),
            ));
        }
    }
    Ok(ordered)
}

/// (column name, is integer) from the SQLite declaration; everything else is copied as text.
async fn columns(pool: &AnyPool, table: &str) -> Result<Vec<(String, bool)>> {
    let rows = sqlx::query(&format!("PRAGMA table_info(\"{table}\")"))
        .fetch_all(pool)
        .await?;
    rows.iter()
        .map(|row| {
            let name: String = row.try_get("name")?;
            let declared: String = row.try_get("type")?;
            let upper = declared.to_ascii_uppercase();
            Ok((name, upper.contains("INT")))
        })
        .collect()
}
