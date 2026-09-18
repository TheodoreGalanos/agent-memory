//! ABOUTME: Tests for administrator backups: a consistent SQLite snapshot, artifact copy and a manifest
//! ABOUTME: recording recovery position, schema version, artifact inventory and the deletion registry.
use chrono::{Duration, Utc};
use memory_domain::{
    contracts::{Authority, ReasonCode, Scope, SourceRef},
    coordination::{HostRequest, HostResponse},
    sources::{SourceKind, SourceVersion},
};
use memory_host::{Credential, Host, Role};
use memory_store::Store;
use uuid::Uuid;

#[tokio::test]
async fn administrators_take_consistent_backups_with_a_manifest() {
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
    let credential = |name: &str, role: Role, scope: Scope| Credential {
        token: name.repeat(32),
        role,
        authority: Authority {
            scope,
            ..auth.clone()
        },
        expires_at: Utc::now() + Duration::hours(1),
    };
    let host = Host::new(
        store.clone(),
        vec![
            credential("a", Role::Administrator, Scope::default()),
            credential("c", Role::Client, scope.clone()),
        ],
        temp.path().join("artifacts"),
    )
    .await
    .unwrap();
    let bearer = |name: &str| format!("Bearer {}", name.repeat(32));
    // A source snapshot gives the backup an artifact to inventory.
    let source_id = Uuid::now_v7();
    host.handle(
        Some(&bearer("c")),
        HostRequest::IngestSource {
            source: SourceVersion {
                reference: SourceRef {
                    source_id,
                    revision: "1".into(),
                },
                label: "Document".into(),
                scope: scope.clone(),
                kind: SourceKind::Document,
                owner: "test".into(),
                acquired_at: Utc::now(),
                acquisition_method: "test".into(),
                precedence: None,
                snapshot_artifact: None,
            },
            content: serde_json::Value::String("line\n".repeat(10)),
        },
    )
    .await
    .unwrap();

    let directory = temp.path().join("backups").join("first");
    assert_eq!(
        host.handle(
            Some(&bearer("c")),
            HostRequest::Backup {
                directory: directory.clone()
            }
        )
        .await
        .unwrap_err()
        .code,
        ReasonCode::ForbiddenScope
    );
    let HostResponse::Backup { manifest } = host
        .handle(
            Some(&bearer("a")),
            HostRequest::Backup {
                directory: directory.clone(),
            },
        )
        .await
        .unwrap()
    else {
        panic!("expected a backup manifest");
    };
    assert_eq!(manifest.database.kind, "sqlite");
    assert!(directory.join(&manifest.database.file).is_file());
    assert!(manifest.database.bytes > 0 && manifest.database.sha256.len() == 64);
    assert!(manifest.schema_version >= 11);
    assert!(manifest.recovery_position > 0);
    assert!(manifest.artifacts.files >= 1 && manifest.artifacts.bytes > 0);
    assert!(directory.join(&manifest.artifacts.root).is_dir());
    assert!(directory.join("manifest.json").is_file());
    assert!(directory.join("deletions.json").is_file());
    assert_eq!(manifest.deletions, 0);

    // The snapshot opens on its own and holds the same source version.
    let restored = Store::connect(&format!(
        "sqlite://{}?mode=rw",
        directory.join(&manifest.database.file).display()
    ))
    .await
    .unwrap();
    let versions = restored.source_versions(&auth, source_id).await.unwrap();
    assert_eq!(versions.len(), 1);

    // A backup never overwrites an existing directory.
    assert_eq!(
        host.handle(Some(&bearer("a")), HostRequest::Backup { directory })
            .await
            .unwrap_err()
            .code,
        ReasonCode::InvalidPayload
    );
}
