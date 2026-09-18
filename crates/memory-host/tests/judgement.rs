use chrono::{Duration, Utc};
use memory_domain::{contracts::*, coordination::*, records::*};
use memory_host::{Credential, Host, Role};
use memory_store::{Store, artifacts::ArtifactService};
use uuid::Uuid;

#[tokio::test]
async fn semantic_judgement_through_http_and_pi() {
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
    let root = directory.path().join("artifacts");
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
    let path = directory.path().join("judgement-fixture.json");
    std::fs::write(&path, serde_json::to_vec(&serde_json::json!({"assignment":assigned,"url":url,"token":token,"directory":directory.path()})).unwrap()).unwrap();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("npx")
            .args([
                "vitest",
                "run",
                "packages/judgement/test/host-runtime.test.ts",
            ])
            .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .env("MEMORY_JUDGEMENT_FIXTURE", path)
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
        &std::fs::read(directory.path().join("judgement-result.json")).unwrap(),
    )
    .unwrap();
    let assessment_id = Uuid::parse_str(result["assessment"].as_str().unwrap()).unwrap();
    assert!(store.assessment(&auth, assessment_id).await.is_ok());
    let usage = store
        .budget_usage(&auth, job.spec.brief.limits.root_budget_id)
        .await
        .unwrap();
    assert_eq!(usage.unresolved.provider_calls, 43);
    assert_eq!(usage.committed.provider_calls, 0);
    let artifacts = ArtifactService::local(store.clone(), root).await.unwrap();
    let evidence_id = Uuid::parse_str(result["evidence"].as_str().unwrap()).unwrap();
    let evidence = artifacts.inspect(&auth, evidence_id).await.unwrap();
    artifacts
        .revoke(&auth, evidence_id, evidence.revision)
        .await
        .unwrap();
    assert!(
        store.assessment(&auth, assessment_id).await.is_err(),
        "revocation must block assessed-result reuse"
    );
}
