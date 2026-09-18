use chrono::{Duration, Utc};
use memory_domain::{contracts::*, coordination::*, records::*};
use memory_host::{Credential, Host, Role};
use memory_store::{Store, artifacts::ArtifactService};
use uuid::Uuid;

#[tokio::test]
async fn formation_through_http_and_pi() {
    let directory = tempfile::tempdir().unwrap();
    let sqlite_url = format!(
        "sqlite://{}?mode=rwc",
        directory.path().join("store.sqlite").display()
    );
    let store = Store::connect(&std::env::var("MEMORY_TEST_POSTGRES_URL").unwrap_or(sqlite_url))
        .await
        .unwrap();
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../evals/property-location/inputs/before-correction.json"
    ))
    .unwrap();
    let mut brief: WorkBrief =
        serde_json::from_value(fixture["command"]["payload"].clone()).unwrap();
    brief.process = Process::Formation;
    brief.scope.entity_ids.clear();
    brief.scope.source_versions.clear();
    let auth = Authority {
        tenant_id: Uuid::now_v7(),
        actor_id: Uuid::now_v7(),
        scope: brief.scope.clone(),
    };
    let policy: MemoryPolicy = serde_json::from_value(serde_json::json!({
        "retention_purpose":"test", "allowed_uses":[], "source_rules":[], "evidence_requirements":[], "applicability_rules":[],
        "budget_class":"test", "scheduling_priority":0, "judgement_dispositions":[], "notification_policy":"quiet", "qualification_requirements":[]
    })).unwrap();
    brief.policy = store
        .create_policy(
            &auth,
            "Test",
            auth.scope.clone(),
            policy,
            ValidTime::Unknown,
        )
        .await
        .unwrap()
        .reference;
    brief.inputs = WorkInputs {
        sources: vec![],
        memories: vec![],
        artifacts: vec![],
    };
    brief.capabilities.sources.clear();
    brief.capabilities.tools = vec!["read".into()];
    brief.limits.max_output_bytes = 20_000_000.try_into().unwrap();
    brief.limits.max_tokens = 20_000_000.try_into().unwrap();
    brief.limits.max_provider_attempts = 100.try_into().unwrap();
    let root = directory.path().join("artifacts");
    let artifacts = ArtifactService::local(store.clone(), &root).await.unwrap();
    let sources =
        memory_store::sources::SourceService::new(store.clone(), artifacts.clone(), 1_048_576)
            .unwrap();
    let source_id = Uuid::now_v7();
    let mut capture: serde_json::Value =
        serde_json::from_str(include_str!("../../../evals/formation/capture.json")).unwrap();
    let native = sources
        .ingest(
            &auth,
            memory_domain::sources::SourceVersion {
                reference: SourceRef {
                    source_id: Uuid::now_v7(),
                    revision: "C".into(),
                },
                label: "Native model".into(),
                scope: auth.scope.clone(),
                kind: memory_domain::sources::SourceKind::Model,
                owner: "model connector".into(),
                acquired_at: Utc::now(),
                acquisition_method: "Test model".into(),
                precedence: None,
                snapshot_artifact: None,
            },
            &memory_store::sources::ModelAdapter,
            br#"{"entities":{"WT-12":{"properties":{"FireResistance":"120 min"}}}}"#,
        )
        .await
        .unwrap();
    let native_locator = store
        .create_source_locator(
            &auth,
            memory_domain::sources::SourceLocator {
                id: Uuid::now_v7(),
                source: native.reference.clone(),
                locator: memory_domain::sources::Locator::Model {
                    entity: "WT-12".into(),
                    property: "FireResistance".into(),
                },
            },
        )
        .await
        .unwrap();
    capture["events"][2]["content"]["source_locators"] = serde_json::json!([native_locator.id]);
    for index in [4, 5] {
        capture["events"][index]["content"]["actor_id"] = serde_json::json!(brief.scope.user_id);
    }
    let mut references = Vec::new();
    for (revision, events) in [
        (
            "before",
            capture["events"].as_array().unwrap()[..2].to_vec(),
        ),
        ("after", capture["events"].as_array().unwrap().clone()),
    ] {
        let source = sources
            .ingest(
                &auth,
                memory_domain::sources::SourceVersion {
                    reference: SourceRef {
                        source_id,
                        revision: revision.into(),
                    },
                    label: "Property capture".into(),
                    scope: auth.scope.clone(),
                    kind: memory_domain::sources::SourceKind::ToolEvents,
                    owner: "test connector".into(),
                    acquired_at: Utc::now(),
                    acquisition_method: "Authored formation fixture".into(),
                    precedence: None,
                    snapshot_artifact: None,
                },
                &memory_store::sources::ToolEventAdapter,
                &serde_json::to_vec(
                    &serde_json::json!({"schema_version":"memory-tool-events/1","events":events}),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        references.push(source.reference);
    }
    // A later correction with an unknown target must roll back earlier writes in its batch.
    let mut broken = capture["events"].as_array().unwrap()[..2].to_vec();
    broken[1]["content"]["corrects"] = serde_json::json!(["missing-event"]);
    let bad = sources
        .ingest(
            &auth,
            memory_domain::sources::SourceVersion {
                reference: SourceRef {
                    source_id: Uuid::now_v7(),
                    revision: "bad".into(),
                },
                label: "Broken correction".into(),
                scope: auth.scope.clone(),
                kind: memory_domain::sources::SourceKind::ToolEvents,
                owner: "test connector".into(),
                acquired_at: Utc::now(),
                acquisition_method: "Rollback fixture".into(),
                precedence: None,
                snapshot_artifact: None,
            },
            &memory_store::sources::ToolEventAdapter,
            &serde_json::to_vec(
                &serde_json::json!({"schema_version":"memory-tool-events/1","events":broken}),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    references.push(bad.reference);
    references.push(native.reference);
    for (revision, events) in [
        ("repeat", capture["events"].clone()),
        ("independent", {
            let mut event = capture["events"][0].clone();
            event["event_id"] = serde_json::json!("instance-again");
            event["content"]["episode"] = serde_json::json!("Independent later query");
            serde_json::json!([event])
        }),
    ] {
        let source = sources
            .ingest(
                &auth,
                memory_domain::sources::SourceVersion {
                    reference: SourceRef {
                        source_id,
                        revision: revision.into(),
                    },
                    label: "Property capture".into(),
                    scope: auth.scope.clone(),
                    kind: memory_domain::sources::SourceKind::ToolEvents,
                    owner: "test connector".into(),
                    acquired_at: Utc::now(),
                    acquisition_method: "Capture revision".into(),
                    precedence: None,
                    snapshot_artifact: None,
                },
                &memory_store::sources::ToolEventAdapter,
                &serde_json::to_vec(
                    &serde_json::json!({"schema_version":"memory-tool-events/1","events":events}),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        references.push(source.reference);
    }
    brief.inputs.sources = references.clone();
    brief.capabilities.sources = references.clone();
    brief.evidence_cutoff = chrono::DateTime::parse_from_rfc3339("2026-09-17T00:03:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let deadline = Utc::now() + Duration::minutes(5);
    store
        .create_budget(
            &auth,
            Budget {
                id: brief.limits.root_budget_id,
                scope: auth.scope.clone(),
                limit: Resources {
                    output_bytes: 20_000_000,
                    tokens: 20_000_000,
                    provider_calls: 100,
                    ..Default::default()
                },
                final_result_reserve: Resources {
                    output_bytes: 8192,
                    ..Default::default()
                },
                deadline,
                max_child_depth: 0,
                max_child_concurrency: 0,
                pricing_revision: "test".into(),
            },
        )
        .await
        .unwrap();
    let job = store
        .submit_job(
            &auth,
            Command {
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
                deadline,
                budget_id: brief.limits.root_budget_id,
                lease_epoch: None,
                payload: SubmitJob {
                    brief,
                    parent_id: None,
                    max_attempts: 3,
                    retain_until: deadline + Duration::days(1),
                },
            },
        )
        .await
        .unwrap();
    let assigned = store.claim_job(&auth, job.id, 300).await.unwrap();
    let permit = Fence {
        job_id: job.id,
        owner_id: auth.actor_id,
        epoch: assigned.epoch,
    };
    store.start_job(&auth, &permit).await.unwrap();
    let token = "test-token".repeat(5);
    let host = Host::new(
        store.clone(),
        vec![Credential {
            token: token.clone(),
            authority: auth.clone(),
            role: Role::Worker { job_id: job.id },
            expires_at: deadline,
        }],
        &root,
    )
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let serving = tokio::spawn(async move {
        axum::serve(listener, memory_host::http::router(host))
            .await
            .unwrap();
    });
    let path = directory.path().join("formation-fixture.json");
    std::fs::write(&path, serde_json::to_vec(&serde_json::json!({"assignment":assigned,"url":url,"token":token,"directory":directory.path(),"sources":references})).unwrap()).unwrap();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("npx")
            .args([
                "vitest",
                "run",
                "packages/formation/test/host-runtime.test.ts",
            ])
            .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .env("MEMORY_FORMATION_FIXTURE", path)
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    serving.abort();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(
        &std::fs::read(directory.path().join("formation-result.json")).unwrap(),
    )
    .unwrap();
    let memories = store
        .memories(
            &auth,
            &MemoryQuery {
                include_inactive: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        memories.len(),
        9,
        "Two initial records, six selected later records and one independent observation"
    );
    let inference = memories
        .iter()
        .find(|v| v.record.label.ends_with("possibly-absent"))
        .unwrap();
    assert_eq!(
        inference.record.evidential_status,
        EvidentialStatus::Inference
    );
    let relation = store
        .relations(
            &auth,
            inference.reference.memory_id,
            &MemoryQuery {
                include_inactive: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(
        relation
            .iter()
            .any(|r| r.relation.kind == RelationKind::Challenges)
    );
    assert_eq!(
        store
            .formation_cursor(&auth, &references[2], "rollback", &job.spec.brief.policy)
            .await
            .unwrap(),
        0
    );
    let window = Uuid::parse_str(result["after"].as_str().unwrap()).unwrap();
    assert!(
        store
            .formation_window(&auth, &permit, window)
            .await
            .unwrap()
            .is_some()
    );
}
