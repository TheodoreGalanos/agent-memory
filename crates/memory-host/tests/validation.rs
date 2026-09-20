//! ABOUTME: Tests for recording an independent output-validator's verdict on a task before its
//! ABOUTME: result commits: only the task's own worker may record it, and clients can read it back.
use chrono::{DateTime, Duration, Utc};
use memory_domain::{
    contracts::{Authority, Command, Process, WireVersion, WorkBrief},
    coordination::{
        Budget, Fence, HostRequest, HostResponse, Resources, SubmitJob, ValidationVerdict,
    },
};
use memory_host::{Credential, Host, Role};
use memory_store::Store;
use uuid::Uuid;

fn fixture() -> (Authority, WorkBrief, DateTime<Utc>) {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../evals/property-location/inputs/before-correction.json"
    ))
    .unwrap();
    let mut brief: WorkBrief =
        serde_json::from_value(fixture["command"]["payload"].clone()).unwrap();
    brief.inputs.sources.clear();
    brief.capabilities.sources.clear();
    brief.scope.source_versions.clear();
    brief.scope.task_id = None;
    let auth = Authority {
        tenant_id: Uuid::now_v7(),
        actor_id: Uuid::now_v7(),
        scope: brief.scope.clone(),
    };
    (auth, brief, Utc::now() + Duration::minutes(10))
}

async fn submit(
    store: &Store,
    auth: &Authority,
    brief: &WorkBrief,
    deadline: DateTime<Utc>,
    process: Process,
) -> Uuid {
    let mut brief = brief.clone();
    brief.process = process;
    let task = Uuid::now_v7();
    brief.task_id = task;
    brief.scope.task_id = Some(task);
    let scope = brief.scope.clone();
    store
        .submit_job(
            auth,
            Command {
                request_id: Uuid::now_v7(),
                command_version: WireVersion::V1,
                tenant_id: auth.tenant_id,
                actor_id: auth.actor_id,
                scope,
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
                    max_attempts: 2,
                    retain_until: deadline + Duration::days(1),
                },
            },
        )
        .await
        .unwrap()
        .id
}

#[tokio::test]
async fn only_the_task_s_own_worker_records_a_verdict_and_clients_read_it_back() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::connect(&format!(
        "sqlite://{}?mode=rwc",
        temp.path().join("host.sqlite").display()
    ))
    .await
    .unwrap();
    let (auth, mut brief, deadline) = fixture();
    brief.limits.root_budget_id = Uuid::now_v7();
    let policy = serde_json::from_value(serde_json::json!({
        "retention_purpose":"test","allowed_uses":[],"source_rules":[],"evidence_requirements":[],"applicability_rules":[],
        "budget_class":"test","scheduling_priority":0,"judgement_dispositions":[],"notification_policy":"quiet","qualification_requirements":[]
    })).unwrap();
    brief.policy = store
        .create_policy(
            &auth,
            "Test",
            auth.scope.clone(),
            policy,
            memory_domain::records::ValidTime::Unknown,
        )
        .await
        .unwrap()
        .reference;
    store
        .create_budget(
            &auth,
            Budget {
                id: brief.limits.root_budget_id,
                scope: auth.scope.clone(),
                limit: Resources {
                    tokens: 1_000_000,
                    provider_calls: 50,
                    output_bytes: 1_000_000,
                    ..Default::default()
                },
                final_result_reserve: Resources {
                    output_bytes: 1024,
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
    let job = submit(&store, &auth, &brief, deadline, Process::Investigation).await;
    let other_job = submit(&store, &auth, &brief, deadline, Process::Investigation).await;
    let assignment = store.claim_job(&auth, job, 60).await.unwrap();
    let fence = Fence {
        job_id: job,
        owner_id: assignment.owner_id,
        epoch: assignment.epoch,
    };
    let other_assignment = store.claim_job(&auth, other_job, 60).await.unwrap();
    let other_fence = Fence {
        job_id: other_job,
        owner_id: other_assignment.owner_id,
        epoch: other_assignment.epoch,
    };
    let host = Host::new(
        store.clone(),
        vec![
            Credential {
                token: "w".repeat(32),
                role: Role::Worker { job_id: job },
                authority: auth.clone(),
                expires_at: deadline,
            },
            Credential {
                token: "o".repeat(32),
                role: Role::Worker { job_id: other_job },
                authority: auth.clone(),
                expires_at: deadline,
            },
            Credential {
                token: "c".repeat(32),
                role: Role::Client,
                authority: auth.clone(),
                expires_at: deadline,
            },
        ],
        temp.path().join("artifacts"),
    )
    .await
    .unwrap();
    let worker = format!("Bearer {}", "w".repeat(32));
    let other_worker = format!("Bearer {}", "o".repeat(32));
    let client = format!("Bearer {}", "c".repeat(32));

    // Neither a client nor another job's worker may record a verdict against this fence.
    assert!(
        host.handle(
            Some(&client),
            HostRequest::RecordValidation {
                fence: fence.clone(),
                verdict: ValidationVerdict::Passed,
                reasons: vec![],
            },
        )
        .await
        .is_err()
    );
    assert!(
        host.handle(
            Some(&other_worker),
            HostRequest::RecordValidation {
                fence: fence.clone(),
                verdict: ValidationVerdict::Passed,
                reasons: vec![],
            },
        )
        .await
        .is_err()
    );
    // A blank reason is rejected; the verdict is not recorded from it.
    assert!(
        host.handle(
            Some(&worker),
            HostRequest::RecordValidation {
                fence: fence.clone(),
                verdict: ValidationVerdict::Failed,
                reasons: vec!["   ".into()],
            },
        )
        .await
        .is_err()
    );
    let HostResponse::Job { job: unrecorded } = host
        .handle(Some(&client), HostRequest::InspectJob { job_id: job })
        .await
        .unwrap()
    else {
        panic!("expected the job");
    };
    assert!(unrecorded.validation.is_none());

    let HostResponse::Job { job: recorded } = host
        .handle(
            Some(&worker),
            HostRequest::RecordValidation {
                fence: fence.clone(),
                verdict: ValidationVerdict::Failed,
                reasons: vec!["Output criteria B3 clearance was not covered".into()],
            },
        )
        .await
        .unwrap()
    else {
        panic!("expected the updated job");
    };
    let validation = recorded.validation.expect("verdict recorded");
    assert_eq!(validation.verdict, ValidationVerdict::Failed);
    assert_eq!(
        validation.reasons,
        vec!["Output criteria B3 clearance was not covered".to_string()]
    );

    // A client reads the recorded verdict the same way it reads anything else about the job.
    let HostResponse::Job { job: seen } = host
        .handle(Some(&client), HostRequest::InspectJob { job_id: job })
        .await
        .unwrap()
    else {
        panic!("expected the job");
    };
    assert_eq!(seen.validation, Some(validation));

    // A later verdict for the same fence replaces the earlier one; recording never accumulates.
    let HostResponse::Job { job: replaced } = host
        .handle(
            Some(&worker),
            HostRequest::RecordValidation {
                fence,
                verdict: ValidationVerdict::Passed,
                reasons: vec![],
            },
        )
        .await
        .unwrap()
    else {
        panic!("expected the updated job");
    };
    let final_validation = replaced.validation.expect("verdict recorded");
    assert_eq!(final_validation.verdict, ValidationVerdict::Passed);
    assert!(final_validation.reasons.is_empty());

    // The other job's own worker still records its own verdict against its own fence.
    assert!(
        host.handle(
            Some(&other_worker),
            HostRequest::RecordValidation {
                fence: other_fence,
                verdict: ValidationVerdict::Unknown,
                reasons: vec!["Task was blocked before criteria could be assessed".into()],
            },
        )
        .await
        .is_ok()
    );
}
