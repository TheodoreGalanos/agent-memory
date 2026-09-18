//! ABOUTME: Migration test: a seeded SQLite store is copied into a disposable PostgreSQL store and
//! ABOUTME: the same records, versions and commit position are readable there; non-empty targets refuse.
use memory_domain::{
    contracts::{Authority, Scope},
    records::MemoryQuery,
};
use memory_store::{Store, migrate::copy_sqlite_to_postgres};
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires MEMORY_TEST_POSTGRES_URL"]
async fn sqlite_snapshot_migrates_into_an_empty_postgres_store() {
    let url = std::env::var("MEMORY_TEST_POSTGRES_URL").unwrap();
    let temp = tempfile::tempdir().unwrap();
    let source_url = format!(
        "sqlite://{}?mode=rwc",
        temp.path().join("source.sqlite").display()
    );
    let source = Store::connect(&source_url).await.unwrap();
    let auth = Authority {
        tenant_id: Uuid::now_v7(),
        actor_id: Uuid::now_v7(),
        scope: Scope {
            project_id: Some(Uuid::now_v7()),
            ..Default::default()
        },
    };
    let policy = serde_json::from_value(serde_json::json!({
        "retention_purpose":"test","allowed_uses":[],"source_rules":[],"evidence_requirements":[],"applicability_rules":[],
        "budget_class":"test","scheduling_priority":0,"judgement_dispositions":[],"notification_policy":"quiet","qualification_requirements":[]
    })).unwrap();
    let reference = source
        .create_policy(
            &auth,
            "Test",
            auth.scope.clone(),
            policy,
            memory_domain::records::ValidTime::Unknown,
        )
        .await
        .unwrap()
        .reference;
    let draft: memory_domain::records::RecordDraft = serde_json::from_value(serde_json::json!({
        "label":"Migrated","scope":auth.scope,"content":{"family":"knowledge","statement":"Carried across","subject":null,"predicate":null,"uncertainty":[],"examined_coverage":[]},
        "origin":"observed","evidential_status":"attributed_statement","availability":"routine","qualification":{"status":"candidate"},"valid_time":{"kind":"unknown"},
        "source_locators":[],"derived_from":[],"decision":{"policy":reference,"action":"retain","reason":"test","constraints":[],"required_evidence":[],"expires_at":null}
    })).unwrap();
    let created = source
        .user_request(
            &auth,
            memory_domain::interaction::UserRequest::Mutate {
                request_id: Uuid::now_v7(),
                mutation: Box::new(memory_domain::interaction::UserMutation::Contribute {
                    record: Box::new(draft),
                }),
            },
        )
        .await
        .unwrap();
    let memory_domain::interaction::UserResponse::Memory { memory, .. } = created else {
        panic!()
    };
    let (position, _) = source.backup_position().await.unwrap();
    assert!(position > 0);

    // The disposable cluster's default database may hold other suites' data; use a fresh database.
    let admin = sqlx::AnyPool::connect(&url).await.unwrap();
    let name = format!("migrate_{}", Uuid::now_v7().simple());
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&admin)
        .await
        .unwrap();
    let target_url = url.replace("/postgres?", &format!("/{name}?"));
    let report = copy_sqlite_to_postgres(&source_url, &target_url)
        .await
        .unwrap();
    assert!(report.rows > 0);
    assert_eq!(u64::try_from(report.commit_sequence).unwrap(), position);
    assert!(
        report
            .tables
            .iter()
            .any(|t| t.table == "memory_versions" && t.rows >= 1)
    );

    let target = Store::connect(&target_url).await.unwrap();
    let records = target
        .memories(&auth, &MemoryQuery::default())
        .await
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].reference, memory.reference);
    let (target_position, _) = target.backup_position().await.unwrap();
    assert_eq!(target_position, position);
    // A target with content refuses a second import.
    assert!(matches!(
        copy_sqlite_to_postgres(&source_url, &target_url).await,
        Err(memory_store::Error::Conflict)
    ));
}
