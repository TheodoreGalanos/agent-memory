//! ABOUTME: Tests for the operational endpoints: liveness, readiness (database reachable) and
//! ABOUTME: Prometheus-format metrics with job counts by state, without tenant identifiers.
use chrono::{Duration, Utc};
use memory_domain::contracts::{Authority, Scope};
use memory_host::{Credential, Host, Role};
use memory_store::Store;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

async fn get(address: &str, path: &str) -> (u16, String) {
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    let status: u16 = response.split_whitespace().nth(1).unwrap().parse().unwrap();
    let body = response.split("\r\n\r\n").nth(1).unwrap_or("").to_owned();
    (status, body)
}

#[tokio::test]
async fn liveness_readiness_and_metrics_are_served_without_credentials() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::connect(&format!(
        "sqlite://{}?mode=rwc",
        temp.path().join("host.sqlite").display()
    ))
    .await
    .unwrap();
    let auth = Authority {
        tenant_id: Uuid::now_v7(),
        actor_id: Uuid::now_v7(),
        scope: Scope::default(),
    };
    let host = Host::new(
        store,
        vec![Credential {
            token: "a".repeat(32),
            role: Role::Administrator,
            authority: auth.clone(),
            expires_at: Utc::now() + Duration::hours(1),
        }],
        temp.path().join("artifacts"),
    )
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let serving = tokio::spawn(async move {
        axum::serve(listener, memory_host::http::router(host))
            .await
            .unwrap();
    });
    let (status, body) = get(&address, "/healthz").await;
    assert_eq!(status, 200);
    assert_eq!(body.trim(), "ok");
    let (status, body) = get(&address, "/readyz").await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("database"));
    let (status, body) = get(&address, "/metrics").await;
    assert_eq!(status, 200);
    assert!(body.contains("memory_jobs_total{state=\"queued\"} 0"));
    assert!(body.contains("memory_host_uptime_seconds"));
    assert!(body.contains("memory_issued_credentials"));
    assert!(
        !body.contains(&auth.tenant_id.to_string()),
        "metrics carry no tenant identifiers"
    );
    serving.abort();
}
