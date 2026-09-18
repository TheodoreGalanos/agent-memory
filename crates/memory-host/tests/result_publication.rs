use chrono::{Duration, Utc};
use memory_domain::{contracts::*, coordination::*, records::*};
use memory_host::{Credential, Host, Role};
use memory_store::{Store, artifacts::ArtifactService};
use uuid::Uuid;

#[tokio::test]
async fn publishes_result_before_completion_and_reuses_its_receipt() {
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
    brief.capabilities.tools = vec!["read".into()];
    brief.limits.max_tokens = 1_000_000.try_into().unwrap();
    brief.limits.max_provider_attempts = 4.try_into().unwrap();
    let deadline = Utc::now() + Duration::minutes(5);
    store
        .create_budget(
            &auth,
            Budget {
                id: brief.limits.root_budget_id,
                scope: auth.scope.clone(),
                limit: Resources {
                    output_bytes: 1_000_000,
                    tokens: 1_000_000,
                    provider_calls: 4,
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
    let assigned = store.claim_job(&auth, job.id, 30).await.unwrap();
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
    let result = WorkResult {
        schema_version: WireVersion::V1,
        status: WorkStatus::Complete,
        examined_scope: auth.scope.clone(),
        inputs: WorkInputs {
            sources: vec![],
            memories: vec![],
            artifacts: vec![],
        },
        findings: vec![],
        coverage: Coverage {
            examined: vec!["test".into()],
            unexamined: vec![],
        },
        unresolved_work: vec![],
        proposed_changes: vec![],
        child_outputs: vec![],
        result_artifact: None,
        known_effects: vec![],
        usage: Usage {
            status: UsageStatus::Unknown,
            input_tokens: None,
            output_tokens: None,
            cost: None,
        },
    };
    let request_id = Uuid::now_v7();
    let request = HostRequest::Complete {
        request_id,
        fence: permit,
        result: Box::new(result),
    };
    let authorization = format!("Bearer {token}");
    let HostResponse::Job { job: first } = host
        .handle(Some(&authorization), request.clone())
        .await
        .unwrap()
    else {
        panic!("Expected completed job")
    };
    assert_eq!(first.state, JobState::Completed);
    assert_eq!(
        first.result.as_ref().unwrap().result_artifact,
        Some(request_id)
    );
    let artifacts = ArtifactService::local(store.clone(), root).await.unwrap();
    let artifact = artifacts.inspect(&auth, request_id).await.unwrap();
    let payload = artifacts
        .read(&auth, request_id, 0..artifact.spec.expected_bytes)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&payload).unwrap(),
        serde_json::to_value(first.result.unwrap()).unwrap()
    );
    let HostResponse::Job { job: repeated } =
        host.handle(Some(&authorization), request).await.unwrap()
    else {
        panic!("Expected receipt")
    };
    assert_eq!(repeated.id, job.id);
    assert_eq!(
        store
            .budget_usage(&auth, job.spec.brief.limits.root_budget_id)
            .await
            .unwrap()
            .committed
            .output_bytes,
        artifact.spec.expected_bytes
    );
    // Exercise the complete wire boundary with a real Pi harness and native read tool.
    let second = store
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
                budget_id: job.spec.brief.limits.root_budget_id,
                lease_epoch: None,
                payload: job.spec.clone(),
            },
        )
        .await
        .unwrap();
    let assignment = store.claim_job(&auth, second.id, 30).await.unwrap();
    let server = Host::new(
        store.clone(),
        vec![Credential {
            token: token.clone(),
            authority: auth.clone(),
            role: Role::Worker { job_id: second.id },
            expires_at: deadline,
        }],
        directory.path().join("artifacts"),
    )
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let serving = tokio::spawn(async move {
        axum::serve(listener, memory_host::http::router(server))
            .await
            .unwrap();
    });
    let path = directory.path().join("worker-fixture.json");
    std::fs::write(&path, serde_json::to_vec(&serde_json::json!({ "assignment":assignment, "url":url, "token":token, "directory":directory.path() })).unwrap()).unwrap();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("npx")
            .args([
                "vitest",
                "run",
                "packages/pi-worker/test/host-worker.test.ts",
            ])
            .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .env("MEMORY_WORKER_FIXTURE", path)
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
    assert_eq!(
        store.job(&auth, second.id).await.unwrap().state,
        JobState::Completed
    );
    // WP07: a parent releases its worker, two independent Pi sessions investigate,
    // and a new parent attempt aggregates their published results.
    let mut scoped_brief = job.spec.brief.clone();
    scoped_brief.limits.root_budget_id = Uuid::now_v7();
    scoped_brief.limits.max_child_depth = 2;
    scoped_brief.limits.max_child_concurrency = 2;
    scoped_brief.limits.max_output_bytes = 65536.try_into().unwrap();
    let definition = br#"{"definitions":{"clearance":"At least 3 metres, including temporary works"},"measurements":[4,2]}"#;
    let artifact = artifacts
        .allocate(
            &auth,
            memory_domain::sources::ArtifactSpec {
                label: "Investigation definitions".into(),
                scope: auth.scope.clone(),
                media_type: "application/json".into(),
                expected_bytes: definition.len() as u32,
                origin: Origin::Observed,
                retention_class: scoped_brief.retention_policy.clone(),
                dependencies: vec![],
            },
        )
        .await
        .unwrap();
    artifacts
        .upload(&auth, artifact.id, artifact.revision, &definition[..])
        .await
        .unwrap();
    scoped_brief.inputs.artifacts = vec![artifact.id];
    store
        .create_budget(
            &auth,
            Budget {
                id: scoped_brief.limits.root_budget_id,
                scope: auth.scope.clone(),
                limit: Resources {
                    tokens: 1_000_000,
                    provider_calls: 4,
                    output_bytes: 1_000_000,
                    ..Default::default()
                },
                final_result_reserve: Resources {
                    output_bytes: 8192,
                    ..Default::default()
                },
                deadline,
                max_child_depth: 2,
                max_child_concurrency: 2,
                pricing_revision: "test".into(),
            },
        )
        .await
        .unwrap();
    let scoped = store
        .submit_job(
            &auth,
            Command {
                command_version: WireVersion::V1,
                request_id: Uuid::now_v7(),
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
                budget_id: scoped_brief.limits.root_budget_id,
                lease_epoch: None,
                payload: SubmitJob {
                    brief: scoped_brief,
                    parent_id: None,
                    max_attempts: 3,
                    retain_until: deadline + Duration::days(1),
                },
            },
        )
        .await
        .unwrap();
    let assignment = store.claim_job(&auth, scoped.id, 30).await.unwrap();
    let worker_token = "investigation-worker-token".repeat(3);
    let scheduler_token = "investigation-scheduler-token".repeat(3);
    let server = Host::new(
        store.clone(),
        vec![
            Credential {
                token: worker_token.clone(),
                authority: auth.clone(),
                role: Role::Worker { job_id: scoped.id },
                expires_at: deadline,
            },
            Credential {
                token: scheduler_token.clone(),
                authority: auth.clone(),
                role: Role::Administrator,
                expires_at: deadline,
            },
        ],
        directory.path().join("artifacts"),
    )
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let serving = tokio::spawn(async move {
        axum::serve(listener, memory_host::http::router(server))
            .await
            .unwrap();
    });
    let path = directory.path().join("investigation-fixture.json");
    std::fs::write(
        &path,
        serde_json::to_vec(
            &serde_json::json!({ "assignment":assignment, "url":url, "token":worker_token,
        "schedulerToken":scheduler_token, "directory":directory.path() }),
        )
        .unwrap(),
    )
    .unwrap();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("npx")
            .args([
                "vitest",
                "run",
                "packages/pi-worker/test/investigation-worker.test.ts",
            ])
            .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .env("MEMORY_INVESTIGATION_FIXTURE", path)
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
    let completed = store.job(&auth, scoped.id).await.unwrap();
    assert_eq!(completed.state, JobState::Partial);
    assert!(
        completed
            .result
            .unwrap()
            .findings
            .iter()
            .any(|f| f.statement.contains("2 metres"))
    );
}
