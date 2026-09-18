//! ABOUTME: Command-line local-to-production migration: copies a SQLite backup snapshot into an
//! ABOUTME: empty PostgreSQL store and prints the verified per-table report as JSON.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os().skip(1);
    let (Some(source), Some(target)) = (arguments.next(), arguments.next()) else {
        return Err(
            "Usage: memory-migrate sqlite://SNAPSHOT.db?mode=rw postgresql://TARGET".into(),
        );
    };
    let report = memory_store::migrate::copy_sqlite_to_postgres(
        source.to_str().ok_or("source URL must be UTF-8")?,
        target.to_str().ok_or("target URL must be UTF-8")?,
    )
    .await?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
