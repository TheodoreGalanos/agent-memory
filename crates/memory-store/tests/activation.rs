use chrono::{Duration, Utc};
use memory_domain::{activation::*, contracts::*, records::*, sources::Entity};
use memory_store::{Error, Store};
use uuid::Uuid;

async fn seed(store: &Store) -> (Authority, PolicyVersion) {
    let auth = Authority {
        tenant_id: Uuid::now_v7(),
        actor_id: Uuid::now_v7(),
        scope: Scope {
            project_id: Some(Uuid::now_v7()),
            ..Default::default()
        },
    };
    let policy:MemoryPolicy=serde_json::from_value(serde_json::json!({"retention_purpose":"activation tests","allowed_uses":["retrieval"],"source_rules":[],"evidence_requirements":[],"applicability_rules":[],"budget_class":"test","scheduling_priority":0,"judgement_dispositions":[],"notification_policy":"quiet","qualification_requirements":[]})).unwrap();
    let policy = store
        .create_policy(
            &auth,
            "test",
            auth.scope.clone(),
            policy,
            ValidTime::Unknown,
        )
        .await
        .unwrap();
    (auth, policy)
}
fn draft(auth: &Authority, policy: &PolicyVersion, text: &str) -> RecordDraft {
    serde_json::from_value(serde_json::json!({"label":text,"scope":auth.scope,"content":{"family":"knowledge","statement":text,"subject":null,"predicate":null,"uncertainty":[],"examined_coverage":[]},"origin":"observed","evidential_status":"observation","availability":"routine","qualification":{"status":"candidate"},"valid_time":{"kind":"unknown"},"source_locators":[],"derived_from":[],"decision":{"policy":policy.reference,"action":"retain","reason":"test support","constraints":[],"required_evidence":[],"expires_at":null}})).unwrap()
}
fn query(auth: &Authority, text: &str) -> ActivationQuery {
    ActivationQuery {
        scope: auth.scope.clone(),
        question: text.into(),
        task_context: "current project".into(),
        entities: vec![],
        exact: vec![],
        families: vec![],
        valid_at: None,
        recorded_as_of: None,
        fresh_after: None,
        vector: None,
        existing: vec![],
        scan_limit: 100,
        candidate_limit: 10,
        traversal_limit: 20,
        context_bytes: 16000,
    }
}
fn vector(values: Vec<f64>) -> Embedding {
    Embedding {
        identity: EmbeddingIdentity {
            model: "authored-test-vectors".into(),
            revision: "1".into(),
            dimensions: 2,
            representation: "memory-content-v1".into(),
        },
        values,
    }
}
async fn suite(store: &Store) {
    let (auth, policy) = seed(store).await;
    let a = store
        .create_memory(&auth, draft(&auth, &policy, "Pump isolation procedure"))
        .await
        .unwrap();
    let b = store
        .create_memory(
            &auth,
            draft(&auth, &policy, "Cavitation exception requires inspection"),
        )
        .await
        .unwrap();
    store
        .create_relation(
            &auth,
            RelationDraft {
                from: a.reference.clone(),
                to: b.reference.clone(),
                kind: RelationKind::Challenges,
                scope: auth.scope.clone(),
                basis: "exception to isolation".into(),
                evidential_status: EvidentialStatus::Observation,
                acceptance: Acceptance::Accepted,
                valid_time: ValidTime::Unknown,
            },
        )
        .await
        .unwrap();
    let (foreign, fp) = seed(store).await;
    let secret = store
        .create_memory(
            &foreign,
            draft(&foreign, &fp, "Pump isolation procedure private"),
        )
        .await
        .unwrap();
    let mut other = auth.clone();
    other.scope.project_id = Some(Uuid::now_v7());
    let op = store
        .create_policy(
            &other,
            "other",
            other.scope.clone(),
            policy.policy.clone(),
            ValidTime::Unknown,
        )
        .await
        .unwrap();
    let other_record = store
        .create_memory(&other, draft(&other, &op, "Pump isolation private project"))
        .await
        .unwrap();
    store
        .save_embedding(&auth, &a.reference, vector(vec![1., 0.]))
        .await
        .unwrap();
    store
        .save_embedding(&auth, &b.reference, vector(vec![0.8, 0.6]))
        .await
        .unwrap();
    store
        .save_embedding(&foreign, &secret.reference, vector(vec![1., 0.]))
        .await
        .unwrap();
    store
        .save_embedding(&other, &other_record.reference, vector(vec![1., 0.]))
        .await
        .unwrap();
    let mut q = query(&auth, "Pump");
    q.vector = Some(vector(vec![1., 0.]));
    let result = store
        .activation(&auth, Uuid::now_v7(), q.clone(), None)
        .await
        .unwrap();
    assert_eq!(result.eligible.len(), 2);
    assert_eq!(result.candidates.len(), 2);
    assert_eq!(
        result.candidates[0].memory.reference.memory_id,
        a.reference.memory_id
    );
    assert!(result.candidates[0].channels.contains(&"lexical".into()));
    assert!(result.candidates[0].channels.contains(&"vector".into()));
    assert!(result.groups[0].unresolved_conflict && result.groups[0].complete);
    assert_eq!(result.groups[0].members.len(), 2);
    let serialized = serde_json::to_string(&result).unwrap();
    assert!(!serialized.contains(&secret.reference.memory_id.to_string()));
    assert!(!serialized.contains(&other_record.reference.memory_id.to_string()));
    let updates = store.embedding_inputs(&auth, None, 1).await.unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].memory.memory_id, a.reference.memory_id);
    assert!(updates[0].text.contains("Pump isolation"));
    let next_updates = store
        .embedding_inputs(&auth, Some(updates[0].cursor.clone()), 100)
        .await
        .unwrap();
    assert_eq!(next_updates.len(), 1);
    assert_eq!(next_updates[0].memory.memory_id, b.reference.memory_id);
    assert!(
        store
            .embedding_inputs(&auth, Some(next_updates[0].cursor.clone()), 100)
            .await
            .unwrap()
            .is_empty()
    );
    // Filtered vector recall is measured against the supplied exact vectors, not a global top-k.
    let mut vq = query(&auth, "unmatchedword");
    vq.vector = Some(vector(vec![1., 0.]));
    let actual = store
        .activation(&auth, Uuid::now_v7(), vq.clone(), None)
        .await
        .unwrap();
    assert_eq!(
        actual
            .candidates
            .iter()
            .filter(|c| c.channels.contains(&"vector".into()))
            .count(),
        2
    );
    vq.vector.as_mut().unwrap().identity.revision = "different".into();
    let mismatched = store
        .activation(&auth, Uuid::now_v7(), vq, None)
        .await
        .unwrap();
    assert!(mismatched.candidates.is_empty());
    assert!(
        mismatched
            .coverage
            .iter()
            .any(|c| c.contains("matching current embedding"))
    );
    // Current writes are searchable immediately, even without embedding refresh.
    let mut replacement = draft(&auth, &policy, "Pump corrected isolation evidence");
    let revised = store
        .revise_memory(&auth, &a.reference, replacement.clone())
        .await
        .unwrap();
    let fresh = store
        .activation(&auth, Uuid::now_v7(), q.clone(), None)
        .await
        .unwrap();
    assert!(
        fresh
            .candidates
            .iter()
            .all(|c| c.memory.version_id != a.version_id)
    );
    assert!(
        fresh
            .candidates
            .iter()
            .any(|c| c.memory.version_id == revised.version_id)
    );
    assert!(matches!(
        store
            .save_embedding(&auth, &a.reference, vector(vec![1., 0.]))
            .await,
        Err(Error::Conflict)
    ));
    // An explicit historical query can inspect the previous revision while it remains available.
    let mut historical = q.clone();
    historical.recorded_as_of = Some(a.recorded.sequence);
    let past = store
        .activation(&auth, Uuid::now_v7(), historical, None)
        .await
        .unwrap();
    assert!(
        past.candidates
            .iter()
            .any(|c| c.memory.version_id == a.version_id)
    );
    replacement.availability = Availability::Retired;
    store
        .revise_memory(&auth, &revised.reference, replacement)
        .await
        .unwrap();
    let retired = store
        .activation(&auth, Uuid::now_v7(), q, None)
        .await
        .unwrap();
    assert!(
        retired
            .candidates
            .iter()
            .all(|c| c.memory.reference.memory_id != a.reference.memory_id)
    );
    assert!(retired.groups.iter().any(|g| !g.complete));

    let entity = Entity {
        id: Uuid::now_v7(),
        scope: auth.scope.clone(),
        provider: "asset-register".into(),
        native_id: "P-42".into(),
        label: "Supply pump".into(),
        aliases: vec!["pump42".into()],
    };
    store.create_entity(&auth, entity.clone()).await.unwrap();
    let mut ed = draft(&auth, &policy, "unrelated wording");
    ed.scope.entity_ids.push(entity.id);
    let linked = store.create_memory(&auth, ed).await.unwrap();
    let mut eq = query(&auth, "unmatchedword");
    eq.entities.push(entity.id);
    eq.scan_limit = 1;
    assert_eq!(
        store.find_entities(&auth, "P-42").await.unwrap()[0].id,
        entity.id
    );
    assert_eq!(
        store
            .activation(&auth, Uuid::now_v7(), eq, None)
            .await
            .unwrap()
            .candidates[0]
            .memory
            .version_id,
        linked.version_id
    );
    let mut exact = query(&auth, "unmatchedword");
    exact.scan_limit = 1;
    exact.exact.push(linked.reference.memory_id);
    assert!(
        store
            .activation(&auth, Uuid::now_v7(), exact, None)
            .await
            .unwrap()
            .candidates
            .iter()
            .any(|c| c.memory.version_id == linked.version_id)
    );
    // Stable scan cursors reject query changes and report bounded coverage.
    let mut pq = query(&auth, "unmatchedword");
    pq.scan_limit = 1;
    let first = store
        .activation(&auth, Uuid::now_v7(), pq.clone(), None)
        .await
        .unwrap();
    assert!(first.next.is_some());
    let second = store
        .activation(&auth, Uuid::now_v7(), pq.clone(), first.next.clone())
        .await
        .unwrap();
    assert_ne!(first.eligible[0].memory_id, second.eligible[0].memory_id);
    pq.question = "changed".into();
    assert!(matches!(
        store
            .activation(&auth, Uuid::now_v7(), pq, first.next)
            .await,
        Err(Error::Invalid(_))
    ));
    let mut time = query(&auth, "unrelated");
    time.valid_at = Some(Utc::now());
    assert!(
        store
            .activation(&auth, Uuid::now_v7(), time, None)
            .await
            .unwrap()
            .candidates
            .is_empty()
    );
    let mut future = query(&auth, "unrelated");
    future.fresh_after = Some(Utc::now() + Duration::days(1));
    assert!(
        store
            .activation(&auth, Uuid::now_v7(), future, None)
            .await
            .unwrap()
            .candidates
            .is_empty()
    );
    assert!(vector(vec![0., 0.]).validate().is_err());
    assert!(vector(vec![1.]).validate().is_err());
}
#[tokio::test]
async fn sqlite_activation_contract() {
    let store = Store::connect("sqlite::memory:").await.unwrap();
    suite(&store).await;
    store.close().await;
}
#[tokio::test]
#[ignore = "requires disposable PostgreSQL"]
async fn postgres_activation_contract() {
    let store = Store::connect(&std::env::var("MEMORY_TEST_POSTGRES_URL").expect("test database"))
        .await
        .unwrap();
    suite(&store).await;
    store.close().await;
}

#[tokio::test]
async fn activation_migration_indexes_existing_records() {
    let directory = tempfile::tempdir().unwrap();
    let url = format!(
        "sqlite://{}?mode=rwc",
        directory.path().join("upgrade.sqlite").display()
    );
    let store = Store::connect(&url).await.unwrap();
    let (auth, policy) = seed(&store).await;
    let record = store
        .create_memory(&auth, draft(&auth, &policy, "Legacy bearing inspection"))
        .await
        .unwrap();
    store.close().await;
    // Recreate the pre-WP10 schema in this disposable database, retaining its records.
    let pool = sqlx::AnyPool::connect(&url).await.unwrap();
    for table in [
        "explorations",
        "decision_requests",
        "task_contexts",
        "notification_preferences",
        "notification_deliveries",
        "runtime_controls",
        "deletion_jobs",
        "revoked_resources",
        "resource_exposures",
        "deleted_sessions",
        "intention_occurrences",
        "intention_checks",
        "maintenance_reviews",
        "memory_changes",
        "memory_search",
        "search_entities",
        "search_updates",
        "memory_embeddings",
        "activation_windows",
        "consolidation_windows",
        "consolidation_reviews",
        "qualification_adoptions",
    ] {
        sqlx::query(&format!("DROP TABLE {table}"))
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM schema_migrations WHERE version>=7")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let upgraded = Store::connect(&url).await.unwrap();
    let found = upgraded
        .activation(&auth, Uuid::now_v7(), query(&auth, "bearing"), None)
        .await
        .unwrap();
    assert_eq!(found.candidates[0].memory.version_id, record.version_id);
    assert_eq!(
        upgraded
            .embedding_inputs(&auth, None, 10)
            .await
            .unwrap()
            .len(),
        1
    );
    upgraded.close().await;
}
