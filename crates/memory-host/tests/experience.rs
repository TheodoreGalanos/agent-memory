//! ABOUTME: Tests for publishing a task's experience: a fenced worker turns its transcript into a
//! ABOUTME: tool-event source scoped to the brief, idempotently per attempt, and clients can find it by job.
use chrono::{DateTime, Duration, Utc};
use memory_domain::{
    contracts::{Authority, Command, EvidentialStatus, Origin, Process, WireVersion, WorkBrief},
    coordination::{Budget, Fence, HostRequest, HostResponse, Resources, SubmitJob},
    sources::ToolEvent,
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

fn event(id: &str, kind: &str, origin: Origin, status: EvidentialStatus, text: &str) -> ToolEvent {
    ToolEvent {
        event_id: id.into(),
        observed_at: Utc::now() - Duration::seconds(1),
        kind: kind.into(),
        origin,
        evidential_status: status,
        content: serde_json::json!({ "text": text }),
    }
}

#[tokio::test]
async fn a_worker_publishes_its_experience_once_per_attempt_and_clients_find_it_by_job() {
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
    let formation = submit(&store, &auth, &brief, deadline, Process::Formation).await;
    let assignment = store.claim_job(&auth, job, 60).await.unwrap();
    let fence = Fence {
        job_id: job,
        owner_id: assignment.owner_id,
        epoch: assignment.epoch,
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
    let client = format!("Bearer {}", "c".repeat(32));
    let events = vec![
        event(
            "1-message",
            "message",
            Origin::Observed,
            EvidentialStatus::AttributedStatement,
            "Find the clearance",
        ),
        event(
            "2-tool_call",
            "tool_call",
            Origin::AgentGenerated,
            EvidentialStatus::Inference,
            "read drawing.pdf",
        ),
        event(
            "3-tool_result",
            "tool_result",
            Origin::Observed,
            EvidentialStatus::Observation,
            "Clearance 3 m",
        ),
    ];

    // Nothing published yet: an explicit not-found, not an empty source.
    assert!(
        host.handle(Some(&client), HostRequest::Experience { job_id: job })
            .await
            .is_err()
    );
    // An empty transcript is not an experience.
    assert!(
        host.handle(
            Some(&worker),
            HostRequest::PublishExperience {
                fence: fence.clone(),
                events: vec![]
            }
        )
        .await
        .is_err()
    );

    let HostResponse::Source { source } = host
        .handle(
            Some(&worker),
            HostRequest::PublishExperience {
                fence: fence.clone(),
                events: events.clone(),
            },
        )
        .await
        .unwrap()
    else {
        panic!("expected the published source");
    };
    // The source is keyed by the job and its attempt, scoped to the project rather than the executing
    // task so later tasks can form from it, and ready to read.
    assert_eq!(source.reference.source_id, job);
    assert_eq!(source.reference.revision, "1");
    let brief_scope = store.job(&auth, job).await.unwrap().spec.brief.scope;
    assert_eq!(source.scope.task_id, None);
    assert_eq!(source.scope.project_id, brief_scope.project_id);
    assert!(
        source
            .label
            .contains(&brief_scope.task_id.unwrap().to_string())
    );
    assert!(source.snapshot_artifact.is_some());
    assert_eq!(source.acquisition_method, "pi-session");

    // Publishing again for the same attempt returns the same source without a second snapshot.
    let HostResponse::Source { source: again } = host
        .handle(
            Some(&worker),
            HostRequest::PublishExperience {
                fence: fence.clone(),
                events,
            },
        )
        .await
        .unwrap()
    else {
        panic!("expected the published source");
    };
    assert_eq!(again.snapshot_artifact, source.snapshot_artifact);

    let HostResponse::Source { source: found } = host
        .handle(Some(&client), HostRequest::Experience { job_id: job })
        .await
        .unwrap()
    else {
        panic!("expected the source by job");
    };
    assert_eq!(found.reference, source.reference);
    let events_read = store
        .events(&auth, 0, 100)
        .await
        .unwrap()
        .events
        .into_iter()
        .filter(|e| e.kind == "experience_published")
        .count();
    assert_eq!(events_read, 1);

    // Only a task's own worker can publish, and only for task processes.
    let other = store.claim_job(&auth, formation, 60).await.unwrap();
    let other_fence = Fence {
        job_id: formation,
        owner_id: other.owner_id,
        epoch: other.epoch,
    };
    assert!(
        host.handle(
            Some(&worker),
            HostRequest::PublishExperience {
                fence: other_fence,
                events: vec![event(
                    "x",
                    "message",
                    Origin::Observed,
                    EvidentialStatus::AttributedStatement,
                    "x"
                )]
            }
        )
        .await
        .is_err()
    );
    assert!(
        host.handle(
            Some(&client),
            HostRequest::PublishExperience {
                fence,
                events: vec![event(
                    "x",
                    "message",
                    Origin::Observed,
                    EvidentialStatus::AttributedStatement,
                    "x"
                )]
            }
        )
        .await
        .is_err()
    );
}
