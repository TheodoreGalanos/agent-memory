//! ABOUTME: Tests for the Host-side worker pool supervisor: one child per pool credential,
//! ABOUTME: pool identity passed by environment, bounded restarts and shutdown.
use memory_host::supervisor::{PoolChild, PoolConfig, run_pool};
use std::time::Duration;

#[tokio::test]
async fn pool_children_receive_their_identity_and_are_restarted_until_shutdown() {
    let temp = tempfile::tempdir().unwrap();
    let log = temp.path().join("children.log");
    let config = PoolConfig {
        command: "sh".into(),
        args: vec![
            "-c".into(),
            format!(
                "echo \"$MEMORY_POOL_CLASSES $MEMORY_HOST_URL ${{#MEMORY_POOL_TOKEN}}\" >> {}",
                log.display()
            ),
        ],
        working_directory: None,
        restart_delay_seconds: 0,
    };
    let children = vec![
        PoolChild {
            token: "a".repeat(32),
            classes: vec!["interactive".into()],
        },
        PoolChild {
            token: "b".repeat(40),
            classes: vec!["deferred".into(), "interactive".into()],
        },
    ];
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let pool = tokio::spawn(run_pool(
        config,
        "http://127.0.0.1:7331".into(),
        children,
        stopped,
    ));
    tokio::time::sleep(Duration::from_millis(600)).await;
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(5), pool)
        .await
        .unwrap()
        .unwrap();
    let lines: Vec<String> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    let interactive = lines
        .iter()
        .filter(|l| l.starts_with("interactive http://127.0.0.1:7331 32"))
        .count();
    let deferred = lines
        .iter()
        .filter(|l| l.starts_with("deferred,interactive http://127.0.0.1:7331 40"))
        .count();
    assert!(interactive >= 2, "child restarted after exit: {lines:?}");
    assert!(
        deferred >= 2,
        "each pool credential has its own child: {lines:?}"
    );
    let after = lines.len();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        std::fs::read_to_string(&log).unwrap().lines().count(),
        after,
        "no spawns after shutdown"
    );
}

#[tokio::test]
async fn shutdown_terminates_a_running_child() {
    let temp = tempfile::tempdir().unwrap();
    let pid_file = temp.path().join("pid");
    let config = PoolConfig {
        command: "sh".into(),
        args: vec![
            "-c".into(),
            format!("echo $$ > {}; exec sleep 30", pid_file.display()),
        ],
        working_directory: None,
        restart_delay_seconds: 5,
    };
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let pool = tokio::spawn(run_pool(
        config,
        "http://127.0.0.1:7331".into(),
        vec![PoolChild {
            token: "c".repeat(32),
            classes: vec!["deferred".into()],
        }],
        stopped,
    ));
    tokio::time::sleep(Duration::from_millis(400)).await;
    let pid: i32 = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(5), pool)
        .await
        .unwrap()
        .unwrap();
    let alive = std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .unwrap()
        .success();
    assert!(!alive, "child {pid} should have been terminated");
}
