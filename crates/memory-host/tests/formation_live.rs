use chrono::{Duration, Utc};
use memory_domain::{contracts::*, coordination::*, records::*};
use memory_host::{Credential, Host, Role};
use memory_store::{Store, artifacts::ArtifactService};
use uuid::Uuid;

#[tokio::test]
#[ignore = "paid opt-in: MEMORY_LIVE_FORMATION=1; uses Azure and Jev"]
async fn live_formation_evaluation() {
    assert_eq!(std::env::var("MEMORY_LIVE_FORMATION").as_deref(), Ok("1"));
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
    brief.limits.max_provider_attempts = 40.try_into().unwrap();
    let root = directory.path().join("artifacts");
    let artifacts = ArtifactService::local(store.clone(), &root).await.unwrap();
    let sources =
        memory_store::sources::SourceService::new(store.clone(), artifacts, 1_048_576).unwrap();
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("../../../evals/formation/live/cases.json")).unwrap();
    let mut references = Vec::new();
    for case in cases["cases"].as_array().unwrap() {
        let mut events = case["events"].clone();
        for event in events.as_array_mut().unwrap() {
            if event["content"]["explicit_contribution"] == true {
                event["content"]["actor_id"] = serde_json::json!(brief.scope.user_id);
            }
        }
        let source = sources
            .ingest(
                &auth,
                memory_domain::sources::SourceVersion {
                    reference: SourceRef {
                        source_id: Uuid::now_v7(),
                        revision: "evaluation-1".into(),
                    },
                    label: case["id"].as_str().unwrap().into(),
                    scope: auth.scope.clone(),
                    kind: memory_domain::sources::SourceKind::ToolEvents,
                    owner: "Authored synthetic evaluation".into(),
                    acquired_at: Utc::now(),
                    acquisition_method: "Evaluation capture".into(),
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
    let deadline = Utc::now() + Duration::minutes(30);
    store
        .create_budget(
            &auth,
            Budget {
                id: brief.limits.root_budget_id,
                scope: auth.scope.clone(),
                limit: Resources {
                    output_bytes: 20_000_000,
                    tokens: 20_000_000,
                    provider_calls: 40,
                    cost_microunits: 1_000_000,
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
    let path = directory.path().join("formation-live-fixture.json");
    std::fs::write(&path, serde_json::to_vec(&serde_json::json!({"assignment":assigned,"url":url,"token":token,"directory":directory.path(),"sources":references})).unwrap()).unwrap();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("npx")
            .args([
                "vitest",
                "run",
                "packages/formation/test/live-runtime.test.ts",
            ])
            .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .env("MEMORY_FORMATION_LIVE_FIXTURE", path)
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
    let records = store
        .memories(
            &auth,
            &MemoryQuery {
                include_inactive: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let path = std::env::var("MEMORY_FORMATION_LIVE_REPORT").expect("report path required");
    let mut report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    report["persistedRecords"] = serde_json::to_value(records).unwrap();
    report["hostBudget"] = serde_json::to_value(
        store
            .budget_usage(&auth, job.spec.brief.limits.root_budget_id)
            .await
            .unwrap(),
    )
    .unwrap();
    std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}
