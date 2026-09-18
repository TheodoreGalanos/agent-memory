//! ABOUTME: HTTP-level tests for publishing source revisions through the authenticated Host.
//! ABOUTME: Covers connector ingestion, scope/role limits, adapter validation and size limits.
use chrono::{Duration, Utc};
use memory_domain::{
    contracts::{Authority, ReasonCode, Scope, SourceRef},
    coordination::{HostRequest, HostResponse},
    sources::{SourceKind, SourceVersion},
};
use memory_host::{Credential, Host, Role};
use memory_store::Store;
use uuid::Uuid;

fn version(scope: &Scope, kind: SourceKind, revision: &str) -> SourceVersion {
    SourceVersion {
        reference: SourceRef {
            source_id: Uuid::now_v7(),
            revision: revision.into(),
        },
        label: "Connector capture".into(),
        scope: scope.clone(),
        kind,
        owner: "Test connector".into(),
        acquired_at: Utc::now(),
        acquisition_method: "HTTP ingestion test".into(),
        precedence: None,
        snapshot_artifact: None,
    }
}

#[tokio::test]
async fn connectors_publish_scoped_source_revisions_over_http() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::connect(&format!(
        "sqlite://{}?mode=rwc",
        temp.path().join("host.sqlite").display()
    ))
    .await
    .unwrap();
    let scope = Scope {
        project_id: Some(Uuid::now_v7()),
        ..Default::default()
    };
    let auth = Authority {
        tenant_id: Uuid::now_v7(),
        actor_id: Uuid::now_v7(),
        scope: scope.clone(),
    };
    let credential = |name: &str, role: Role| Credential {
        token: name.repeat(32),
        role,
        authority: auth.clone(),
        expires_at: Utc::now() + Duration::hours(1),
    };
    let host = Host::new(
        store.clone(),
        vec![
            credential("c", Role::Client),
            credential(
                "w",
                Role::Worker {
                    job_id: Uuid::now_v7(),
                },
            ),
        ],
        temp.path().join("artifacts"),
    )
    .await
    .unwrap();
    let bearer = |name: &str| format!("Bearer {}", name.repeat(32));
    let client = Some(bearer("c"));
    let capture: serde_json::Value =
        serde_json::from_str(include_str!("../../../evals/formation/capture.json")).unwrap();
    let events = serde_json::json!({
        "schema_version": "memory-tool-events/1",
        "events": capture["events"].as_array().unwrap()[..2].to_vec(),
    });

    let published = host
        .handle(
            client.as_deref(),
            HostRequest::IngestSource {
                source: version(&scope, SourceKind::ToolEvents, "1"),
                content: events.clone(),
            },
        )
        .await
        .unwrap();
    let HostResponse::Source { source } = published else {
        panic!("expected a source response");
    };
    let snapshot = source
        .snapshot_artifact
        .expect("snapshot artifact recorded");
    let stored = store
        .source_version(&auth, &source.reference)
        .await
        .unwrap();
    assert_eq!(stored.snapshot_artifact, Some(snapshot));
    assert_eq!(stored.kind, SourceKind::ToolEvents);

    // A text document is sent as a JSON string and stored as UTF-8 text.
    let document = host
        .handle(
            client.as_deref(),
            HostRequest::IngestSource {
                source: version(&scope, SourceKind::Document, "1"),
                content: serde_json::Value::String("line one\nline two\n".into()),
            },
        )
        .await
        .unwrap();
    assert!(matches!(document, HostResponse::Source { .. }));

    // The service assigns the snapshot; a caller cannot point at an existing artifact.
    let mut preassigned = version(&scope, SourceKind::ToolEvents, "1");
    preassigned.snapshot_artifact = Some(snapshot);
    assert_eq!(
        host.handle(
            client.as_deref(),
            HostRequest::IngestSource {
                source: preassigned,
                content: events.clone(),
            }
        )
        .await
        .unwrap_err()
        .code,
        ReasonCode::InvalidPayload
    );

    // Tool events must satisfy the adapter contract.
    assert_eq!(
        host.handle(
            client.as_deref(),
            HostRequest::IngestSource {
                source: version(&scope, SourceKind::ToolEvents, "1"),
                content: serde_json::json!({"schema_version": "other", "events": []}),
            }
        )
        .await
        .unwrap_err()
        .code,
        ReasonCode::InvalidPayload
    );

    // A source outside the credential's scope is refused before any artifact is written.
    let elsewhere = Scope {
        project_id: Some(Uuid::now_v7()),
        ..Default::default()
    };
    assert_eq!(
        host.handle(
            client.as_deref(),
            HostRequest::IngestSource {
                source: version(&elsewhere, SourceKind::ToolEvents, "1"),
                content: events.clone(),
            }
        )
        .await
        .unwrap_err()
        .code,
        ReasonCode::ForbiddenScope
    );

    // Oversized snapshots are refused.
    let large = serde_json::Value::String("x".repeat(1_048_577));
    assert_eq!(
        host.handle(
            client.as_deref(),
            HostRequest::IngestSource {
                source: version(&scope, SourceKind::Document, "1"),
                content: large,
            }
        )
        .await
        .unwrap_err()
        .code,
        ReasonCode::BudgetExhausted
    );

    // Workers execute assigned jobs; they do not publish new sources.
    assert_eq!(
        host.handle(
            Some(&bearer("w")),
            HostRequest::IngestSource {
                source: version(&scope, SourceKind::ToolEvents, "2"),
                content: events,
            }
        )
        .await
        .unwrap_err()
        .code,
        ReasonCode::ForbiddenScope
    );
}
