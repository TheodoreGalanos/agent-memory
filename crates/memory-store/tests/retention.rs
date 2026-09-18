use chrono::{Duration, Utc};
use memory_domain::{contracts::*, coordination::*, records::*, retention::*, sources::*};
use memory_store::{Store, artifacts::ArtifactService};
use sqlx::Row;
use uuid::Uuid;

async fn setup(store: &Store) -> (Authority, Command<SubmitJob>) {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../evals/property-location/inputs/before-correction.json"
    ))
    .unwrap();
    let mut seed: Command<WorkBrief> = serde_json::from_value(fixture["command"].clone()).unwrap();
    let auth = Authority {
        tenant_id: Uuid::now_v7(),
        actor_id: Uuid::now_v7(),
        scope: Scope {
            task_id: Some(Uuid::now_v7()),
            project_id: Some(Uuid::now_v7()),
            ..Default::default()
        },
    };
    let policy = store
        .create_policy(
            &auth,
            "Test retention",
            auth.scope.clone(),
            MemoryPolicy {
                semantic_triggers: vec![],
                retention_purpose: "Continuation".into(),
                allowed_uses: vec![],
                source_rules: vec![],
                evidence_requirements: vec![],
                applicability_rules: vec![],
                budget_class: "test".into(),
                scheduling_priority: 0,
                judgement_dispositions: vec![],
                notification_policy: "quiet".into(),
                qualification_requirements: vec![],
                consolidation: None,
            },
            ValidTime::Unknown,
        )
        .await
        .unwrap();
    let budget = store
        .create_budget(
            &auth,
            Budget {
                id: Uuid::now_v7(),
                scope: auth.scope.clone(),
                limit: Resources {
                    tokens: 100000,
                    provider_calls: 100,
                    output_bytes: 1000000,
                    ..Default::default()
                },
                final_result_reserve: Resources {
                    tokens: 10,
                    output_bytes: 128,
                    ..Default::default()
                },
                deadline: Utc::now() + Duration::hours(1),
                max_child_depth: 2,
                max_child_concurrency: 1,
                pricing_revision: "test/no-charge".into(),
            },
        )
        .await
        .unwrap();
    seed.payload.task_id = auth.scope.task_id.unwrap();
    seed.payload.scope = auth.scope.clone();
    seed.payload.policy = policy.reference;
    seed.payload.limits.root_budget_id = budget.id;
    seed.payload.inputs = WorkInputs {
        memories: vec![],
        sources: vec![],
        artifacts: vec![],
    };
    seed.payload.capabilities.sources.clear();
    seed.payload.limits.max_child_depth = 2;
    seed.payload.limits.max_child_concurrency = 1;
    seed.payload.evidence_cutoff = Utc::now();
    let command = Command {
        request_id: Uuid::now_v7(),
        command_version: WireVersion::V1,
        tenant_id: auth.tenant_id,
        actor_id: auth.actor_id,
        scope: auth.scope.clone(),
        job_id: None,
        session_id: None,
        operation_id: None,
        lane_id: None,
        invocation_id: None,
        expected_revisions: vec![],
        deadline: Utc::now() + Duration::minutes(30),
        budget_id: budget.id,
        lease_epoch: None,
        payload: SubmitJob {
            brief: seed.payload,
            parent_id: None,
            max_attempts: 5,
            retain_until: Utc::now() + Duration::days(1),
        },
    };
    (auth, command)
}
fn permit(assignment: &Assignment) -> Fence {
    Fence {
        job_id: assignment.job.id,
        owner_id: assignment.owner_id,
        epoch: assignment.epoch,
    }
}
fn record(auth: &Authority, policy: &ConfigRef) -> RecordDraft {
    RecordDraft {
        label: "Inspection result".into(),
        scope: auth.scope.clone(),
        content: MemoryContent::Knowledge {
            statement: "Property appears on the type".into(),
            subject: None,
            predicate: None,
            uncertainty: vec![],
            examined_coverage: vec![],
        },
        origin: Origin::AgentGenerated,
        evidential_status: EvidentialStatus::Inference,
        availability: Availability::Routine,
        qualification: Qualification::Candidate,
        valid_time: ValidTime::Unknown,
        source_locators: vec![],
        derived_from: vec![],
        decision: PolicyDecision {
            policy: policy.clone(),
            action: PolicyAction::Retain,
            reason: "Useful continuation".into(),
            constraints: vec![],
            required_evidence: vec![],
            expires_at: None,
        },
    }
}
async fn exercise(url: &str) {
    let store = Store::connect(url).await.unwrap();
    let (auth, mut command) = setup(&store).await;
    let secret = store
        .create_memory(&auth, record(&auth, &command.payload.brief.policy))
        .await
        .unwrap();
    let mut draft = record(&auth, &command.payload.brief.policy);
    draft.label = "Governed derivative".into();
    draft.derived_from.push(secret.reference.clone());
    let derivative = store.create_memory(&auth, draft).await.unwrap();
    let survivor = store
        .create_memory(&auth, record(&auth, &command.payload.brief.policy))
        .await
        .unwrap();
    store
        .create_relation(
            &auth,
            RelationDraft {
                from: secret.reference.clone(),
                to: survivor.reference.clone(),
                kind: RelationKind::Supports,
                scope: auth.scope.clone(),
                basis: "Removed evidence".into(),
                evidential_status: EvidentialStatus::Inference,
                acceptance: Acceptance::Accepted,
                valid_time: ValidTime::Unknown,
            },
        )
        .await
        .unwrap();
    command
        .payload
        .brief
        .inputs
        .memories
        .push(secret.reference.clone());
    let job = store.submit_job(&auth, command.clone()).await.unwrap();
    let assignment = store.claim_job(&auth, job.id, 30).await.unwrap();
    let effect = store
        .prepare_effect(
            &auth,
            &permit(&assignment),
            EffectRequest {
                logical_operation_id: format!("{}/write-secret", job.operation_id),
                invocation_id: "write-secret".into(),
                kind: "write".into(),
                replay: ReplayClass::Reconcile,
                arguments: serde_json::json!({"text":"secret"}),
            },
        )
        .await
        .unwrap();
    store
        .begin_effect(&auth, &permit(&assignment), effect.id)
        .await
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let artifacts = ArtifactService::local(store.clone(), directory.path())
        .await
        .unwrap();
    let artifact = artifacts
        .allocate(
            &auth,
            ArtifactSpec {
                label: "Session output".into(),
                scope: auth.scope.clone(),
                media_type: "text/plain".into(),
                expected_bytes: 6,
                origin: Origin::AgentGenerated,
                retention_class: "test".into(),
                dependencies: vec![],
            },
        )
        .await
        .unwrap();
    artifacts
        .upload(&auth, artifact.id, artifact.revision, "secret".as_bytes())
        .await
        .unwrap();
    store
        .link_job_artifact(&auth, &permit(&assignment), artifact.id)
        .await
        .unwrap();
    let (other, _) = setup(&store).await;
    assert!(
        store
            .begin_deletion(
                &other,
                DeletionRequest {
                    id: Uuid::now_v7(),
                    resources: vec![secret.reference.memory_id]
                }
            )
            .await
            .is_err()
    );
    let deletion = Uuid::now_v7();
    let report = store
        .begin_deletion(
            &auth,
            DeletionRequest {
                id: deletion,
                resources: vec![secret.reference.memory_id],
            },
        )
        .await
        .unwrap();
    assert!(report.access_denied && !report.live_payloads_removed);
    assert_eq!(report.effects[0].state, EffectState::OutcomeUnknown);
    assert!(report.resources.contains(&derivative.reference.memory_id));
    assert!(report.resources.contains(&job.id));
    assert!(report.resources.contains(&artifact.id));
    assert!(!report.resources.contains(&survivor.reference.memory_id));
    assert!(
        report
            .support_review
            .contains(&survivor.reference.memory_id)
    );
    assert!(store.memory(&auth, &secret.reference).await.is_err());
    assert!(
        store
            .assigned_job(&auth, &permit(&assignment))
            .await
            .is_err()
    );
    assert!(
        store
            .record_exposure(&auth, job.id, &serde_json::json!({}))
            .await
            .is_err()
    );
    assert!(artifacts.read(&auth, artifact.id, 0..6).await.is_err());
    assert!(artifacts.purge_deletion(&auth, deletion).await.is_err());
    for obligation in &report.obligations {
        if matches!(
            obligation.kind,
            RetentionBoundary::Session | RetentionBoundary::Sandbox
        ) {
            store
                .acknowledge_purge(&auth, deletion, obligation.kind, &obligation.container)
                .await
                .unwrap();
        }
    }
    let purged = artifacts.purge_deletion(&auth, deletion).await.unwrap();
    assert!(purged.live_payloads_removed);
    assert!(
        purged
            .obligations
            .iter()
            .any(|o| o.kind == RetentionBoundary::Backup && !o.acknowledged)
    );
    assert!(
        purged
            .obligations
            .iter()
            .any(|o| o.kind == RetentionBoundary::Provider && !o.acknowledged)
    );
    assert!(store.memory(&auth, &survivor.reference).await.is_ok());
    let pending = artifacts
        .allocate(
            &auth,
            ArtifactSpec {
                label: "Interrupted upload".into(),
                scope: auth.scope.clone(),
                media_type: "text/plain".into(),
                expected_bytes: 6,
                origin: Origin::AgentGenerated,
                retention_class: "fixture".into(),
                dependencies: vec![],
            },
        )
        .await
        .unwrap();
    let mut upload = artifacts
        .begin_upload(&auth, pending.id, pending.revision)
        .await
        .unwrap();
    upload.write(b"secret").await.unwrap();
    let removal = store
        .begin_deletion(
            &auth,
            DeletionRequest {
                id: Uuid::now_v7(),
                resources: vec![pending.id],
            },
        )
        .await
        .unwrap();
    assert!(artifacts.purge_deletion(&auth, removal.id).await.is_err());
    upload.abort().await.unwrap();
    store
        .acknowledge_purge(
            &auth,
            removal.id,
            RetentionBoundary::Upload,
            &pending.id.to_string(),
        )
        .await
        .unwrap();
    artifacts.purge_deletion(&auth, removal.id).await.unwrap();
    let pool = sqlx::AnyPool::connect(url).await.unwrap();
    for table in ["memory_versions", "memory_search", "memory_embeddings"] {
        let n: i64 = sqlx::query(&format!(
            "SELECT COUNT(*) AS n FROM {table} WHERE version_id=$1"
        ))
        .bind(secret.version_id.to_string())
        .fetch_one(&pool)
        .await
        .unwrap()
        .get("n");
        assert_eq!(n, 0, "{table}");
    }
    let output = directory
        .path()
        .join(auth.tenant_id.to_string())
        .join(artifact.id.to_string());
    assert!(!output.exists() || std::fs::read_dir(output).unwrap().next().is_none());
    artifacts.purge_deletion(&auth, deletion).await.unwrap();
    // Fresh admission uses only surviving input references and allocates independent Pi IDs.
    command.request_id = Uuid::now_v7();
    command.payload.brief.inputs.memories = vec![survivor.reference.clone()];
    command.payload.brief.capabilities.tools = vec!["read".into()];
    assert!(
        store
            .continue_deleted_work(&auth, deletion, job.id, command.clone())
            .await
            .is_err()
    );
    let reconciliation = artifacts
        .allocate(
            &auth,
            ArtifactSpec {
                label: "Reconciliation observation".into(),
                scope: auth.scope.clone(),
                media_type: "text/plain".into(),
                expected_bytes: 4,
                origin: Origin::AgentGenerated,
                retention_class: "receipt".into(),
                dependencies: vec![],
            },
        )
        .await
        .unwrap();
    artifacts
        .upload(
            &auth,
            reconciliation.id,
            reconciliation.revision,
            "none".as_bytes(),
        )
        .await
        .unwrap();
    store
        .reconcile_deleted_effect(
            &auth,
            deletion,
            effect.id,
            EffectResolution::NotPerformed,
            reconciliation.id,
        )
        .await
        .unwrap();
    assert!(
        store
            .begin_effect(&auth, &permit(&assignment), effect.id)
            .await
            .is_err()
    );
    let continuation = store
        .continue_deleted_work(&auth, deletion, job.id, command)
        .await
        .unwrap();
    assert_ne!(continuation.session_id, job.session_id);
    assert_ne!(continuation.operation_id, job.operation_id);
    assert!(store.claim_job(&auth, job.id, 30).await.is_err());
    // Applying a deletion registry again resets physical completion; it never reopens access.
    store
        .restore_deletion_registry(&auth, vec![purged])
        .await
        .unwrap();
    assert!(
        !store
            .deletion_report(&auth, deletion)
            .await
            .unwrap()
            .live_payloads_removed
    );
    assert!(store.memory(&auth, &secret.reference).await.is_err());
    // A source deletion covers its locators and retained text, across revisions.
    let source = SourceVersion {
        reference: SourceRef {
            source_id: Uuid::now_v7(),
            revision: "r1".into(),
        },
        label: "Private source".into(),
        scope: auth.scope.clone(),
        kind: SourceKind::Document,
        owner: "fixture".into(),
        acquired_at: Utc::now(),
        acquisition_method: "fixture".into(),
        precedence: None,
        snapshot_artifact: None,
    };
    store
        .register_source_version(&auth, source.clone())
        .await
        .unwrap();
    let locator = SourceLocator {
        id: Uuid::now_v7(),
        source: source.reference.clone(),
        locator: Locator::Document {
            start_line: 1,
            end_line: 1,
            page: None,
        },
    };
    store
        .create_source_locator(&auth, locator.clone())
        .await
        .unwrap();
    let mut source_record = record(&auth, &continuation.spec.brief.policy);
    source_record.source_locators.push(locator.id);
    let source_record = store.create_memory(&auth, source_record).await.unwrap();
    let removal = store
        .begin_deletion(
            &auth,
            DeletionRequest {
                id: Uuid::now_v7(),
                resources: vec![source.reference.source_id],
            },
        )
        .await
        .unwrap();
    assert!(
        removal
            .resources
            .contains(&source_record.reference.memory_id)
    );
    artifacts.purge_deletion(&auth, removal.id).await.unwrap();
    assert!(
        store
            .source_version(&auth, &source.reference)
            .await
            .is_err()
    );
    assert!(store.register_source_version(&auth, source).await.is_err());
    assert!(store.memory(&auth, &survivor.reference).await.is_ok());
    pool.close().await;
    store.close().await;
}
#[tokio::test]
async fn sqlite_erasure() {
    let directory = tempfile::tempdir().unwrap();
    exercise(&format!(
        "sqlite://{}?mode=rwc",
        directory.path().join("store.sqlite").display()
    ))
    .await;
}
#[tokio::test]
#[ignore = "requires disposable PostgreSQL"]
async fn postgres_erasure() {
    exercise(&std::env::var("MEMORY_TEST_POSTGRES_URL").unwrap()).await;
}

#[tokio::test]
async fn sqlite_restore_applies_deletion_before_reads() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("original.sqlite");
    let restored = directory.path().join("restored.sqlite");
    let store = Store::connect(&format!("sqlite://{}?mode=rwc", original.display()))
        .await
        .unwrap();
    let (auth, command) = setup(&store).await;
    let secret = store
        .create_memory(&auth, record(&auth, &command.payload.brief.policy))
        .await
        .unwrap();
    store.close().await;
    std::fs::copy(&original, &restored).unwrap();
    let store = Store::connect(&format!("sqlite://{}", original.display()))
        .await
        .unwrap();
    let report = store
        .begin_deletion(
            &auth,
            DeletionRequest {
                id: Uuid::now_v7(),
                resources: vec![secret.reference.memory_id],
            },
        )
        .await
        .unwrap();
    let restored = Store::connect(&format!("sqlite://{}", restored.display()))
        .await
        .unwrap();
    // Prove this really is a pre-deletion backup before applying the offline restore gate.
    assert!(restored.memory(&auth, &secret.reference).await.is_ok());
    restored
        .restore_deletion_registry(&auth, vec![report])
        .await
        .unwrap();
    assert!(restored.memory(&auth, &secret.reference).await.is_err());
    restored.close().await;
    store.close().await;
}
