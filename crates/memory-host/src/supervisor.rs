//! ABOUTME: Host-side worker pool supervisor for the local profile: one child process per pool
//! ABOUTME: credential, identity passed by environment, bounded restart backoff, stop on shutdown.
use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoolConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub working_directory: Option<PathBuf>,
    /// Delay before restarting an exited child; doubles per consecutive failure up to one minute.
    #[serde(default = "default_restart_delay")]
    pub restart_delay_seconds: u64,
}
fn default_restart_delay() -> u64 {
    5
}

/// One pool credential drives one child. The token reaches the child only through its environment.
#[derive(Clone, Debug)]
pub struct PoolChild {
    pub token: String,
    pub classes: Vec<String>,
}

const MAX_RESTART_DELAY: Duration = Duration::from_secs(60);
const TERMINATION_GRACE: Duration = Duration::from_secs(10);

/// Runs until `shutdown` becomes true, then terminates children and returns.
pub async fn run_pool(
    config: PoolConfig,
    host_url: String,
    children: Vec<PoolChild>,
    shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let mut tasks = tokio::task::JoinSet::new();
    for (index, child) in children.into_iter().enumerate() {
        tasks.spawn(supervise(
            config.clone(),
            host_url.clone(),
            child,
            index,
            shutdown.clone(),
        ));
    }
    while tasks.join_next().await.is_some() {}
}

async fn supervise(
    config: PoolConfig,
    host_url: String,
    child: PoolChild,
    index: usize,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let mut delay = Duration::from_secs(config.restart_delay_seconds);
    loop {
        if *shutdown.borrow() {
            return;
        }
        let mut command = tokio::process::Command::new(&config.command);
        command
            .args(&config.args)
            .env("MEMORY_HOST_URL", &host_url)
            .env("MEMORY_POOL_TOKEN", &child.token)
            .env("MEMORY_POOL_CLASSES", child.classes.join(","))
            .env("MEMORY_POOL_INDEX", index.to_string())
            .kill_on_drop(true);
        if let Some(directory) = &config.working_directory {
            command.current_dir(directory);
        }
        let started = std::time::Instant::now();
        let mut process = match command.spawn() {
            Ok(process) => process,
            Err(error) => {
                eprintln!(
                    "Worker pool {index}: cannot start {}: {error}",
                    config.command
                );
                if wait_or_shutdown(&mut shutdown, delay).await {
                    return;
                }
                delay = (delay * 2)
                    .min(MAX_RESTART_DELAY)
                    .max(Duration::from_secs(1));
                continue;
            }
        };
        tokio::select! {
            status = process.wait() => {
                match status {
                    Ok(status) => eprintln!("Worker pool {index}: child exited with {status}"),
                    Err(error) => eprintln!("Worker pool {index}: child wait failed: {error}"),
                }
            }
            _ = shutdown.changed() => {
                terminate(&mut process).await;
                return;
            }
        }
        // A child that ran for a while gets the configured delay; rapid failures back off.
        if started.elapsed() > Duration::from_secs(60) {
            delay = Duration::from_secs(config.restart_delay_seconds);
        }
        if wait_or_shutdown(&mut shutdown, delay).await {
            return;
        }
        delay = (delay * 2)
            .min(MAX_RESTART_DELAY)
            .max(Duration::from_millis(1));
    }
}

/// Returns true when shutdown was requested during the wait.
async fn wait_or_shutdown(
    shutdown: &mut tokio::sync::watch::Receiver<bool>,
    delay: Duration,
) -> bool {
    if *shutdown.borrow() {
        return true;
    }
    tokio::select! {
        _ = tokio::time::sleep(delay) => false,
        _ = shutdown.changed() => true,
    }
}

async fn terminate(process: &mut tokio::process::Child) {
    #[cfg(unix)]
    if let Some(pid) = process.id() {
        // SIGTERM lets the worker finish or release its current lease cleanly.
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }
    }
    if tokio::time::timeout(TERMINATION_GRACE, process.wait())
        .await
        .is_err()
    {
        let _ = process.kill().await;
    }
}
