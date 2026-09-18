use chrono::{Duration, Utc};
use memory_domain::{contracts::*, coordination::*, records::*, retention::*};
use memory_host::{Credential, Host, Role};
use memory_store::Store;
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

#[tokio::test]
async fn host_erasure_fences_exposed_workers_and_requires_administration() {
    let store = Store::connect("sqlite::memory:").await.unwrap();
    let (auth, command) = setup(&store).await;
    let secret = store
        .create_memory(&auth, record(&auth, &command.payload.brief.policy))
        .await
        .unwrap();
    // The secret is not in the brief: the worker encounters it through a later read.
    let job = store.submit_job(&auth, command).await.unwrap();
    let assigned = store.claim_job(&auth, job.id, 30).await.unwrap();
    let root = tempfile::tempdir().unwrap();
    let admin = "admin-credential-retention-test-12345";
    let worker = "worker-credential-retention-test-1234";
    let host = Host::new(
        store.clone(),
        vec![
            Credential {
                token: admin.into(),
                authority: auth.clone(),
                role: Role::Administrator,
                expires_at: Utc::now() + Duration::hours(1),
            },
            Credential {
                token: worker.into(),
                authority: auth.clone(),
                role: Role::Worker { job_id: job.id },
                expires_at: Utc::now() + Duration::hours(1),
            },
        ],
        root.path(),
    )
    .await
    .unwrap();
    let authorization = format!("Bearer {worker}");
    let fence = Fence {
        job_id: job.id,
        owner_id: assigned.owner_id,
        epoch: assigned.epoch,
    };
    let foreign_auth = Authority {
        scope: Scope {
            project_id: Some(Uuid::now_v7()),
            ..Default::default()
        },
        ..auth.clone()
    };
    let artifacts = memory_store::artifacts::ArtifactService::local(store.clone(), root.path())
        .await
        .unwrap();
    let foreign = artifacts
        .allocate(
            &foreign_auth,
            memory_domain::sources::ArtifactSpec {
                label: "Unrelated project".into(),
                scope: foreign_auth.scope.clone(),
                media_type: "text/plain".into(),
                expected_bytes: 0,
                origin: Origin::AgentGenerated,
                retention_class: "fixture".into(),
                dependencies: vec![],
            },
        )
        .await
        .unwrap();
    assert!(
        host.handle(
            Some(&authorization),
            HostRequest::ReadInput {
                fence: fence.clone(),
                artifact_id: foreign.id,
                offset: 0,
                limit: 0
            }
        )
        .await
        .is_err()
    );
    let foreign_deletion = store
        .begin_deletion(
            &foreign_auth,
            DeletionRequest {
                id: Uuid::now_v7(),
                resources: vec![foreign.id],
            },
        )
        .await
        .unwrap();
    assert!(!foreign_deletion.resources.contains(&job.id));
    host.handle(
        Some(&authorization),
        HostRequest::CurrentMemories {
            fence: fence.clone(),
            references: vec![secret.reference.clone()],
        },
    )
    .await
    .unwrap();
    let request = HostRequest::BeginDeletion {
        request: DeletionRequest {
            id: Uuid::now_v7(),
            resources: vec![secret.reference.memory_id],
        },
    };
    assert!(
        host.handle(Some(&authorization), request.clone())
            .await
            .is_err()
    );
    let response = host
        .handle(Some(&format!("Bearer {admin}")), request)
        .await
        .unwrap();
    let HostResponse::Deletion { report } = response else {
        panic!("wrong response")
    };
    assert!(report.resources.contains(&job.id));
    assert!(
        host.handle(
            Some(&authorization),
            HostRequest::InspectAssignment { fence }
        )
        .await
        .is_err()
    );
    assert!(
        host.handle(
            Some(&authorization),
            HostRequest::InspectJob { job_id: job.id }
        )
        .await
        .is_err()
    );
    store.close().await;
}
