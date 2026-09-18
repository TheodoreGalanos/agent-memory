use chrono::{DateTime, Utc};
use memory_domain::contracts::{Authority, Scope};
use memory_host::{Credential, Host, Role};
use memory_store::Store;
use serde::Deserialize;
use std::{net::SocketAddr, path::PathBuf};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Configuration {
    listen: SocketAddr,
    database_url: String,
    artifact_root: PathBuf,
    credentials: Vec<Identity>,
    #[serde(default)]
    restricted_providers: Vec<String>,
    #[serde(default)]
    restore_registry: Option<PathBuf>,
    /// When present, the host starts one worker pool child per `pool` credential.
    #[serde(default)]
    worker_pool: Option<memory_host::supervisor::PoolConfig>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    token: String,
    tenant_id: Uuid,
    actor_id: Uuid,
    scope: Scope,
    role: Role,
    expires_at: DateTime<Utc>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("Usage: memory-host <private-config.json>")?,
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(&path)?.permissions().mode() & 0o077 != 0 {
            return Err("Host configuration must be private (chmod 600)".into());
        }
    }
    let config: Configuration = serde_json::from_slice(&std::fs::read(path)?)?;
    if !config.listen.ip().is_loopback() {
        return Err("Bind to loopback; remote access requires a TLS reverse proxy".into());
    }
    let store = Store::connect(&config.database_url).await?;
    let credentials: Vec<Credential> = config
        .credentials
        .into_iter()
        .map(|c| Credential {
            token: c.token,
            authority: Authority {
                tenant_id: c.tenant_id,
                actor_id: c.actor_id,
                scope: c.scope,
            },
            role: c.role,
            expires_at: c.expires_at,
        })
        .collect();
    // Restore barriers are applied before the HTTP listener or timers can expose data.
    if let Some(path) = config.restore_registry {
        let reports: Vec<memory_domain::retention::DeletionReport> =
            serde_json::from_slice(&std::fs::read(path)?)?;
        for report in reports {
            let administrator = credentials
                .iter()
                .find(|c| {
                    matches!(c.role, Role::Administrator)
                        && c.expires_at > Utc::now()
                        && c.authority.tenant_id == report.tenant_id
                        && c.authority.scope.permits(&report.scope)
                })
                .ok_or("Restore registry requires an administrator covering each deletion")?;
            store
                .restore_deletion_registry(&administrator.authority, vec![report])
                .await?;
        }
    }
    let timer_scopes: Vec<_> = credentials
        .iter()
        .filter(|c| matches!(c.role, Role::Administrator))
        .map(|c| (c.authority.clone(), c.expires_at))
        .collect();
    let pool_children: Vec<_> = credentials
        .iter()
        .filter_map(|c| match &c.role {
            Role::Pool { classes } => Some(memory_host::supervisor::PoolChild {
                token: c.token.clone(),
                classes: classes
                    .iter()
                    .map(|class| {
                        serde_json::to_value(class)
                            .ok()?
                            .as_str()
                            .map(str::to_owned)
                    })
                    .collect::<Option<Vec<_>>>()
                    .unwrap_or_default(),
            }),
            _ => None,
        })
        .collect();
    let timer_store = store.clone();
    let host = Host::new(store, credentials, config.artifact_root)
        .await
        .map_err(|_| "Invalid host credential configuration")?
        .with_restricted_providers(config.restricted_providers);
    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    println!("Memory host listening on {}", listener.local_addr()?);
    let timers = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut tick: u64 = 0;
        loop {
            interval.tick().await;
            tick += 1;
            for (authority, expires) in &timer_scopes {
                if *expires <= chrono::Utc::now() {
                    continue;
                }
                if timer_store.sweep_intentions(authority, 100).await.is_err() {
                    eprintln!("Intention timer sweep failed; retrying on the next tick");
                }
                // Expired leases return to the queue only through recovery.
                if tick % RECOVERY_TICKS == 0 && timer_store.recover_jobs(authority).await.is_err()
                {
                    eprintln!("Job recovery failed; retrying on the next tick");
                }
            }
        }
    });
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let pool = match config.worker_pool {
        Some(pool_config) if !pool_children.is_empty() => {
            println!("Starting {} worker pool child(ren)", pool_children.len());
            Some(tokio::spawn(memory_host::supervisor::run_pool(
                pool_config,
                format!("http://{}", listener.local_addr()?),
                pool_children,
                stopped,
            )))
        }
        Some(_) => {
            eprintln!(
                "worker_pool is configured but no pool credential exists; no children started"
            );
            None
        }
        None => None,
    };
    let served = axum::serve(listener, memory_host::http::router(host))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    timers.abort();
    let _ = stop.send(true);
    if let Some(pool) = pool {
        let _ = pool.await;
    }
    served?;
    Ok(())
}

/// Recovery runs every five timer ticks; intention sweeps every tick.
const RECOVERY_TICKS: u64 = 5;
