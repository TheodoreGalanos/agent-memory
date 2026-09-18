use chrono::{Duration, Utc};
use memory_domain::{contracts::*, coordination::*, records::*};
use memory_host::{Credential, Host, Role};
use memory_store::Store;
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires the isolated Docker profile and pinned Harbor; run npm run test:docker"]
async fn harbor_sandbox_lifecycle() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::connect(&format!(
        "sqlite://{}?mode=rwc",
        directory.path().join("store.sqlite").display()
    ))
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
    brief.capabilities.tools = vec![
        "read".into(),
        "write".into(),
        "edit".into(),
        "bash".into(),
        "python".into(),
    ];
    brief.limits.max_output_bytes = 65536.try_into().unwrap();
    brief.limits.max_tokens = 1_000_000.try_into().unwrap();
    brief.limits.max_provider_attempts = 8.try_into().unwrap();
    let deadline = Utc::now() + Duration::minutes(10);
    store
        .create_budget(
            &auth,
            Budget {
                id: brief.limits.root_budget_id,
                scope: auth.scope.clone(),
                limit: Resources {
                    output_bytes: 1_000_000,
                    cost_microunits: 1_000_000,
                    sandbox_cpu_ms: 1_000_000,
                    sandbox_time_ms: 1_000_000,
                    tokens: 1_000_000,
                    provider_calls: 8,
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
    let token = "sandbox-test-token".repeat(4);
    let worker_token = "sandbox-worker-token".repeat(4);
    let host = Host::new(
        store.clone(),
        vec![
            Credential {
                token: token.clone(),
                authority: auth.clone(),
                role: Role::Administrator,
                expires_at: deadline,
            },
            Credential {
                token: worker_token.clone(),
                authority: auth.clone(),
                role: Role::Worker { job_id: job.id },
                expires_at: deadline,
            },
        ],
        directory.path().join("artifacts"),
    )
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, memory_host::http::router(host))
            .await
            .unwrap();
    });
    let fixture = directory.path().join("fixture.json");
    std::fs::write(&fixture, serde_json::to_vec(&serde_json::json!({"assignment":assigned,"url":url,"token":worker_token,"admin_token":token,"directory":directory.path()})).unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fixture, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let project = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(project.join(".venv-harbor/bin/python"))
            .arg(project.join("services/harbor-bridge/integration_test.py"))
            .arg(fixture)
            .current_dir(project)
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    server.abort();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(store.job(&auth, job.id).await.unwrap().state.terminal());
    store.close().await;
}
