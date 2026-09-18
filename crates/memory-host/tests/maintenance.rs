use chrono::{Duration, Utc};
use memory_domain::{contracts::*, coordination::*, intentions::*, records::*};
use memory_store::Store;
use uuid::Uuid;

fn semantic_trigger() -> memory_domain::judgement::JudgementDefinition {
    let catalogue: Vec<memory_domain::judgement::JudgementDefinition> = serde_json::from_str(
        include_str!("../../../packages/judgement/src/catalogue-data.json"),
    )
    .unwrap();
    let mut d = catalogue.into_iter().find(|d| d.id == "J16").unwrap();
    d.id = "fixture-trigger".into();
    d.input_requirements = vec!["intention".into(), "event".into()];
    d.permitted_uses = vec!["candidate_intentions".into()];
    d
}
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
                semantic_triggers: vec![semantic_trigger()],
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
    seed.payload.limits.max_tokens = 100000.try_into().unwrap();
    seed.payload.limits.max_output_bytes = 1000000.try_into().unwrap();
    seed.payload.limits.max_provider_attempts = 100.try_into().unwrap();
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
fn result(auth: &Authority) -> WorkResult {
    WorkResult {
        schema_version: WireVersion::V1,
        status: WorkStatus::Complete,
        examined_scope: auth.scope.clone(),
        inputs: WorkInputs {
            memories: vec![],
            sources: vec![],
            artifacts: vec![],
        },
        findings: vec![],
        coverage: Coverage {
            examined: vec!["Fixture".into()],
            unexamined: vec![],
        },
        unresolved_work: vec![],
        proposed_changes: vec![],
        child_outputs: vec![],
        result_artifact: None,
        known_effects: vec![],
        usage: Usage {
            status: UsageStatus::Known,
            input_tokens: Some(0),
            output_tokens: Some(0),
            cost: None,
        },
    }
}

fn definition(auth: &Authority, brief: &WorkBrief) -> RecordDraft {
    let mut r = record(auth, &brief.policy);
    r.content = MemoryContent::Intention {
        purpose: "Verify the corrected property".into(),
        owner_id: auth.actor_id,
        trigger: "Due now".into(),
        readiness: vec![],
        completion: vec!["Checked execution is complete".into()],
        expires_at: Some(Utc::now() + Duration::minutes(10)),
        notification_policy: "quiet".into(),
        recurrence: None,
        plan: Some(Box::new(IntentionPlan {
            trigger: IntentionTrigger::Time {
                at: Utc::now() - Duration::seconds(1),
            },
            execution: Box::new(brief.clone()),
            ready_memories: vec![],
            ready_artifacts: vec![],
            recurrence_seconds: None,
            completion: CompletionRule {
                result_pointer: "/status".into(),
                equals: serde_json::json!("complete"),
                required_effects: vec![],
                semantic_conditions: vec![],
                confirmation_owner: Some(auth.actor_id),
                delivery_grace_seconds: None,
            },
        })),
    };
    r
}

use memory_host::{Credential, Host, Role};
#[tokio::test]
async fn maintenance_and_intentions_through_host_and_pi() {
    let directory = tempfile::tempdir_in("/tmp").unwrap();
    let sqlite = format!(
        "sqlite://{}?mode=rwc",
        directory.path().join("store.sqlite").display()
    );
    let store = Store::connect(&std::env::var("MEMORY_TEST_POSTGRES_URL").unwrap_or(sqlite))
        .await
        .unwrap();
    let (a, mut command) = setup(&store).await;
    let before = store
        .create_memory(&a, record(&a, &command.payload.brief.policy))
        .await
        .unwrap();
    let mut intention = definition(&a, &command.payload.brief);
    if let MemoryContent::Intention {
        readiness,
        plan: Some(p),
        ..
    } = &mut intention.content
    {
        readiness.push("Evidence is available".into());
        p.completion
            .semantic_conditions
            .push("The result verifies the requested condition".into());
    }
    let intention = store.create_memory(&a, intention).await.unwrap();
    let occurrence = store
        .intentions(&a, intention.reference.memory_id)
        .await
        .unwrap()
        .remove(0);
    let mut semantic = definition(&a, &command.payload.brief);
    if let MemoryContent::Intention { plan: Some(p), .. } = &mut semantic.content {
        p.trigger = IntentionTrigger::Semantic {
            definition: Box::new(semantic_trigger()),
            matched_choice: "affected".into(),
        };
    }
    let semantic = store.create_memory(&a, semantic).await.unwrap();
    let semantic_occurrence = store
        .intentions(&a, semantic.reference.memory_id)
        .await
        .unwrap()
        .remove(0);
    command.payload.brief.process = Process::Maintenance;
    command.payload.brief.inputs.memories = vec![
        before.reference.clone(),
        intention.reference.clone(),
        semantic.reference.clone(),
    ];
    let job = store.submit_job(&a, command.clone()).await.unwrap();
    let assignment = store.claim_job(&a, job.id, 300).await.unwrap();
    command.request_id = Uuid::now_v7();
    let next = store.submit_job(&a, command.clone()).await.unwrap();
    command.request_id = Uuid::now_v7();
    command.payload.brief.process = Process::Activation;
    let activation = store.submit_job(&a, command).await.unwrap();
    let activation_worker = "activation-worker-credential-for-fixture".to_string();
    let token = "maintenance-admin-credential-for-fixture".to_string();
    let worker = "maintenance-worker-credential-for-fixture".to_string();
    let next_worker = "maintenance-second-worker-credential-for-fixture".to_string();
    let credentials = [
        (
            activation_worker.clone(),
            Role::Worker {
                job_id: activation.id,
            },
        ),
        (token.clone(), Role::Administrator),
        (worker.clone(), Role::Worker { job_id: job.id }),
        (next_worker.clone(), Role::Worker { job_id: next.id }),
    ]
    .into_iter()
    .map(|(token, role)| Credential {
        token,
        authority: a.clone(),
        role,
        expires_at: Utc::now() + Duration::hours(1),
    })
    .collect();
    let host = Host::new(
        store.clone(),
        credentials,
        directory.path().join("artifacts"),
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
    let path = directory.path().join("fixture.json");
    std::fs::write(&path,serde_json::to_vec(&serde_json::json!({"assignment":assignment,"semantic":semantic_occurrence,"activation_job":activation.id,"activation_worker_token":activation_worker,"next_job":next.id,"before":before,"occurrence":occurrence,"url":url,"token":token,"worker_token":worker,"next_worker_token":next_worker,"directory":directory.path(),"result":result(&a)})).unwrap()).unwrap();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("npx")
            .args([
                "vitest",
                "run",
                "packages/maintenance/test/host-runtime.test.ts",
            ])
            .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .env("MEMORY_MAINTENANCE_FIXTURE", path)
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
    store.close().await;
}
