use chrono::{Duration, Utc};
use memory_domain::{consolidation::*, contracts::*, coordination::*, records::*, sources::*};
use memory_host::{Credential, Host, Role};
use memory_store::{Store, artifacts::ArtifactService};
use uuid::Uuid;

async fn artifact(service: &ArtifactService, auth: &Authority, label: &str, text: &str) -> Uuid {
    let a = service
        .allocate_named(
            auth,
            Uuid::now_v7(),
            ArtifactSpec {
                label: label.into(),
                scope: auth.scope.clone(),
                media_type: "application/json".into(),
                expected_bytes: text.len() as u32,
                origin: Origin::Observed,
                retention_class: "test".into(),
                dependencies: vec![],
            },
        )
        .await
        .unwrap();
    service
        .upload(auth, a.id, a.revision, text.as_bytes())
        .await
        .unwrap();
    a.id
}
#[tokio::test]
async fn consolidation_and_held_out_transfer_through_host_and_pi() {
    fixture(false).await;
}

#[tokio::test]
#[ignore = "paid opt-in: MEMORY_LIVE_CONSOLIDATION=1; Azure and Jev"]
async fn live_consolidation_evaluation() {
    assert_eq!(
        std::env::var("MEMORY_LIVE_CONSOLIDATION").as_deref(),
        Ok("1")
    );
    fixture(true).await;
}

async fn fixture(live: bool) {
    let directory = tempfile::tempdir_in("/tmp").unwrap();
    let sqlite = format!(
        "sqlite://{}?mode=rwc",
        directory.path().join("store.sqlite").display()
    );
    let store = Store::connect(&std::env::var("MEMORY_TEST_POSTGRES_URL").unwrap_or(sqlite))
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
    let mut policy: MemoryPolicy = serde_json::from_value(serde_json::json!({"retention_purpose":"test","allowed_uses":[],"source_rules":[],"evidence_requirements":[],"applicability_rules":[],"budget_class":"test","scheduling_priority":0,"judgement_dispositions":[],"notification_policy":"quiet","qualification_requirements":[]})).unwrap();
    policy.consolidation = Some(ConsolidationPolicy {
        min_independent_sources: 2,
        min_held_out_groups: 2,
        minimum_success_rate: 1.0,
        maximum_regression: 0.0,
        maximum_cost_microunits: if live { 250_000 } else { 1000 },
        evaluator_profile: brief.profile.clone(),
    });
    brief.policy = store
        .create_policy(
            &auth,
            "Consolidation test policy",
            auth.scope.clone(),
            policy,
            ValidTime::Unknown,
        )
        .await
        .unwrap()
        .reference;
    let mut sources = vec![];
    for i in 0..2 {
        let source = SourceVersion {
            reference: SourceRef {
                source_id: Uuid::now_v7(),
                revision: "1".into(),
            },
            label: format!("Independent source {i}"),
            scope: auth.scope.clone(),
            kind: SourceKind::Document,
            owner: "fixture".into(),
            acquired_at: Utc::now(),
            acquisition_method: "authored fixture".into(),
            precedence: None,
            snapshot_artifact: None,
        };
        store
            .register_source_version(&auth, source.clone())
            .await
            .unwrap();
        let locator = SourceLocator {
            id: Uuid::now_v7(),
            source: source.reference,
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
        sources.push(locator);
    }
    let mut cases = vec![];
    for (label, locator, location) in [
        ("Instance observation", sources[0].id, "instance"),
        ("Restated instance observation", sources[0].id, "instance"),
        ("Type convention exception", sources[1].id, "type"),
    ] {
        cases.push(
            store
                .create_memory(
                    &auth,
                    RecordDraft {
                        label: label.into(),
                        scope: auth.scope.clone(),
                        content: MemoryContent::Episode {
                            objective: "Find rated pressure".into(),
                            initial_conditions: vec![format!("{location} convention")],
                            observations: vec![format!("Rated pressure is on the {location}")],
                            actions: vec![format!("Read {location}.rated_pressure")],
                            corrections: vec![],
                            outcome: "Value found with supporting source".into(),
                            verification: vec!["Read the field directly".into()],
                            uncertainty: vec!["Other source conventions remain untested".into()],
                        },
                        origin: Origin::Observed,
                        evidential_status: EvidentialStatus::Observation,
                        availability: Availability::Routine,
                        qualification: Qualification::Candidate,
                        valid_time: ValidTime::Unknown,
                        source_locators: vec![locator],
                        derived_from: vec![],
                        decision: PolicyDecision {
                            policy: brief.policy.clone(),
                            action: PolicyAction::Retain,
                            reason: "Authored cohort".into(),
                            constraints: vec![],
                            required_evidence: vec![],
                            expires_at: None,
                        },
                    },
                )
                .await
                .unwrap(),
        );
    }
    store
        .create_relation(
            &auth,
            RelationDraft {
                from: cases[2].reference.clone(),
                to: cases[0].reference.clone(),
                kind: RelationKind::Challenges,
                scope: auth.scope.clone(),
                basis: "Instance-only rule misses type convention".into(),
                evidential_status: EvidentialStatus::Observation,
                acceptance: Acceptance::Accepted,
                valid_time: ValidTime::Unknown,
            },
        )
        .await
        .unwrap();
    let service = ArtifactService::local(store.clone(), directory.path().join("artifacts"))
        .await
        .unwrap();
    let suite = QualificationSuite {
        evaluator_profile: brief.profile.clone(),
        cases: vec![
            QualificationCase {
                id: "held-out-instance".into(),
                source_group: Uuid::now_v7(),
                conditions: vec!["instance convention".into()],
                task: serde_json::json!({"instance":{"rated_pressure":12},"type":{}}),
                expected: serde_json::json!({"value":12,"location":"instance"}),
            },
            QualificationCase {
                id: "held-out-type".into(),
                source_group: Uuid::now_v7(),
                conditions: vec!["type convention".into()],
                task: serde_json::json!({"instance":{},"type":{"rated_pressure":24}}),
                expected: serde_json::json!({"value":24,"location":"type"}),
            },
        ],
    };
    let suite_id = artifact(
        &service,
        &auth,
        "Protected transfer suite",
        &serde_json::to_string(&suite).unwrap(),
    )
    .await;
    let mut leaked_suite = suite.clone();
    leaked_suite.cases[0].source_group = sources[0].source.source_id;
    let leaked_suite_id = artifact(
        &service,
        &auth,
        "Invalid training-overlap suite",
        &serde_json::to_string(&leaked_suite).unwrap(),
    )
    .await;
    let code = "def find_pressure(instance, type):\n    for location, record in [('instance', instance), ('type', type)]:\n        if 'rated_pressure' in record:\n            return {'value': record['rated_pressure'], 'location': location}\n    return None\n";
    let code_id = artifact(&service, &auth, "Pressure inspection procedure", code).await;
    brief.process = Process::Consolidation;
    brief.evidence_cutoff = Utc::now();
    brief.inputs = WorkInputs {
        sources: vec![],
        memories: cases.iter().map(|c| c.reference.clone()).collect(),
        artifacts: vec![suite_id, leaked_suite_id, code_id],
    };
    brief.capabilities.sources.clear();
    brief.capabilities.tools = vec!["python".into(), "read".into()];
    brief.limits.max_tokens = 20_000_000.try_into().unwrap();
    brief.limits.max_output_bytes = 20_000_000.try_into().unwrap();
    brief.limits.max_provider_attempts = 200.try_into().unwrap();
    brief.limits.max_child_depth = 1;
    brief.limits.max_child_concurrency = 2;
    let deadline = Utc::now() + Duration::minutes(if live { 20 } else { 5 });
    store
        .create_budget(
            &auth,
            Budget {
                id: brief.limits.root_budget_id,
                scope: auth.scope.clone(),
                limit: Resources {
                    tokens: 20_000_000,
                    output_bytes: 20_000_000,
                    provider_calls: if live { 24 } else { 200 },
                    cost_microunits: if live { 1_000_000 } else { 0 },
                    ..Default::default()
                },
                final_result_reserve: Resources {
                    output_bytes: 8192,
                    ..Default::default()
                },
                deadline,
                max_child_depth: 1,
                max_child_concurrency: 2,
                pricing_revision: "test".into(),
            },
        )
        .await
        .unwrap();
    let job = store
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
    let assignment = store.claim_job(&auth, job.id, 300).await.unwrap();
    let fence = Fence {
        job_id: job.id,
        owner_id: auth.actor_id,
        epoch: assignment.epoch,
    };
    store.start_job(&auth, &fence).await.unwrap();
    let token = "consolidation-test-admin".repeat(3);
    let worker_token = "consolidation-test-worker".repeat(3);
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
    let serving = tokio::spawn(async move {
        axum::serve(listener, memory_host::http::router(host))
            .await
            .unwrap();
    });
    let path = directory.path().join("fixture.json");
    std::fs::write(&path,serde_json::to_vec(&serde_json::json!({"assignment":assignment,"url":url,"token":token,"worker_token":worker_token,"directory":directory.path(),"cases":cases.iter().map(|c| &c.reference).collect::<Vec<_>>(),"suite":suite_id,"overlap_suite":leaked_suite_id,"code":code_id})).unwrap()).unwrap();
    let captured = store
        .consolidation_window(
            &auth,
            &Fence {
                job_id: assignment.job.id,
                owner_id: assignment.owner_id,
                epoch: assignment.epoch,
            },
            Uuid::now_v7(),
            CohortSelection {
                purpose: "Check source correction".into(),
                mechanism: "Property lookup".into(),
                outcomes: vec![],
                source_context: "Fixture sources".into(),
                members: vec![cases[0].reference.clone()],
            },
        )
        .await
        .unwrap();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("npx")
            .args([
                "vitest",
                "run",
                if live {
                    "packages/consolidation/test/live-runtime.test.ts"
                } else {
                    "packages/consolidation/test/host-runtime.test.ts"
                },
            ])
            .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .env("MEMORY_CONSOLIDATION_FIXTURE", path)
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
    store
        .revise_memory(&auth, &cases[0].reference, cases[0].record.clone())
        .await
        .unwrap();
    assert!(
        matches!(
            store.check_cohort(&auth, &captured).await,
            Err(memory_store::Error::Unavailable)
        ),
        "Source corrections invalidate the retained cohort"
    );
}
