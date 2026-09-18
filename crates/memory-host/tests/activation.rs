use chrono::{Duration, Utc};
use memory_domain::sources::ArtifactSpec;
use memory_domain::{contracts::*, coordination::*, records::*};
use memory_host::{Credential, Host, Role};
use memory_store::{Store, artifacts::ArtifactService};
use uuid::Uuid;

#[tokio::test]
async fn activation_through_http_pi_and_workspace() {
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
    brief.process = Process::Activation;
    brief.scope.entity_ids.clear();
    brief.scope.source_versions.clear();
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
    let make_record = |label: &str, content: MemoryContent| RecordDraft {
        label: label.into(),
        scope: auth.scope.clone(),
        content,
        origin: Origin::Observed,
        evidential_status: EvidentialStatus::Observation,
        availability: Availability::Routine,
        qualification: Qualification::Candidate,
        valid_time: ValidTime::Unknown,
        source_locators: vec![],
        derived_from: vec![],
        decision: PolicyDecision {
            policy: brief.policy.clone(),
            action: PolicyAction::Retain,
            reason: "Authored activation test".into(),
            constraints: vec![],
            required_evidence: vec![],
            expires_at: None,
        },
    };
    let method = store
        .create_memory(
            &auth,
            make_record(
                "Pump isolation",
                MemoryContent::Procedure {
                    purpose: "Isolate the pump before inspection".into(),
                    capabilities: vec!["isolation".into()],
                    applicability: vec!["Power is disconnected".into()],
                    exclusions: vec!["Bypass valve is open".into()],
                    method: ProcedureForm::Advisory {
                        steps: vec![
                            "Check power and bypass".into(),
                            "Close the inlet valve".into(),
                        ],
                        evidence_criteria: vec!["Local gauge reads zero".into()],
                    },
                    counterexamples: vec![],
                    contract: None,
                },
            ),
        )
        .await
        .unwrap();
    let exception = store
        .create_memory(
            &auth,
            make_record(
                "Pump exception",
                MemoryContent::Knowledge {
                    statement: "An open bypass requires a separate isolation step".into(),
                    subject: None,
                    predicate: None,
                    uncertainty: vec!["Check local bypass state".into()],
                    examined_coverage: vec!["Bypass configuration".into()],
                },
            ),
        )
        .await
        .unwrap();
    store
        .create_relation(
            &auth,
            RelationDraft {
                from: method.reference.clone(),
                to: exception.reference.clone(),
                kind: RelationKind::Challenges,
                scope: auth.scope.clone(),
                basis: "Exception to method".into(),
                evidential_status: EvidentialStatus::Observation,
                acceptance: Acceptance::Accepted,
                valid_time: ValidTime::Unknown,
            },
        )
        .await
        .unwrap();
    let artifacts = ArtifactService::local(store.clone(), directory.path().join("artifacts"))
        .await
        .unwrap();
    let body = b"def calculate():\n    return 42\n";
    let artifact = artifacts
        .allocate_named(
            &auth,
            Uuid::now_v7(),
            ArtifactSpec {
                label: "Torque definition".into(),
                scope: auth.scope.clone(),
                media_type: "text/x-python".into(),
                expected_bytes: body.len() as u32,
                origin: Origin::Observed,
                retention_class: "test".into(),
                dependencies: vec![],
            },
        )
        .await
        .unwrap();
    artifacts
        .upload(&auth, artifact.id, artifact.revision, &body[..])
        .await
        .unwrap();
    let executable = store
        .create_memory(
            &auth,
            make_record(
                "Torque calculation",
                MemoryContent::Procedure {
                    purpose: "Calculate fastener load".into(),
                    capabilities: vec![],
                    applicability: vec![],
                    exclusions: vec![],
                    method: ProcedureForm::Executable {
                        artifact_id: artifact.id,
                        entrypoint: "calculate".into(),
                        inputs: vec![],
                        outputs: vec!["load".into()],
                    },
                    counterexamples: vec![],
                    contract: None,
                },
            ),
        )
        .await
        .unwrap();
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
    let admin_token = "test-admin-token".repeat(4);
    let host = Host::new(
        store.clone(),
        vec![
            Credential {
                token: token.clone(),
                authority: auth.clone(),
                role: Role::Worker { job_id: job.id },
                expires_at: deadline,
            },
            Credential {
                token: admin_token.clone(),
                authority: auth.clone(),
                role: Role::Administrator,
                expires_at: deadline,
            },
        ],
        &root,
    )
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let checking_host = host.clone();
    let serving = tokio::spawn(async move {
        axum::serve(listener, memory_host::http::router(host))
            .await
            .unwrap();
    });
    let credential = format!("Bearer {token}");
    let path = directory.path().join("activation-fixture.json");
    std::fs::write(&path, serde_json::to_vec(&serde_json::json!({"assignment":assigned,"url":url,"token":token,"admin_token":admin_token,"directory":directory.path(),"method":method.reference,"exception":exception.reference,"executable":executable.reference})).unwrap()).unwrap();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("npx")
            .args([
                "vitest",
                "run",
                "packages/activation/test/host-runtime.test.ts",
                "packages/activation/test/nomic.test.ts",
            ])
            .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .env("MEMORY_ACTIVATION_FIXTURE", path)
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
        &std::fs::read(directory.path().join("activation-result.json")).unwrap(),
    )
    .unwrap();
    let mut retired = method.record.clone();
    retired.availability = Availability::Retired;
    store
        .revise_memory(&auth, &method.reference, retired)
        .await
        .unwrap();
    let response = checking_host
        .handle(
            Some(&credential),
            HostRequest::SelectActivation {
                fence: permit,
                selection: memory_domain::activation::ActivationSelection {
                    window_id: result["window"].as_str().unwrap().parse().unwrap(),
                    decisions: Default::default(),
                },
            },
        )
        .await;
    assert_eq!(
        response.unwrap_err().code,
        ReasonCode::SourceUnavailable,
        "Old context must not expose a retired candidate"
    );
}
