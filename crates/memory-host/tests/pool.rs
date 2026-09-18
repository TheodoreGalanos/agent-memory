//! ABOUTME: Tests for the worker pool role: claiming the next eligible job in the pool's work
//! ABOUTME: classes and receiving a job-bound worker credential, or an explicit idle reply.
use chrono::{DateTime, Duration, Utc};
use memory_domain::{
    contracts::{Authority, Command, Process, ReasonCode, Scope, WireVersion, WorkBrief},
    coordination::{Budget, HostRequest, HostResponse, Resources, SubmitJob, WorkClass},
};
use memory_host::{Credential, Host, Role};
use memory_store::Store;
use uuid::Uuid;

struct World {
    store: Store,
    auth: Authority,
    brief: WorkBrief,
    deadline: DateTime<Utc>,
}

async fn world(store: Store) -> World {
    let (auth, brief, deadline) = fixture_world();
    world_in(store, auth, brief, deadline).await
}

fn fixture_world() -> (Authority, WorkBrief, DateTime<Utc>) {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../evals/property-location/inputs/before-correction.json"
    ))
    .unwrap();
    let mut brief: WorkBrief =
        serde_json::from_value(fixture["command"]["payload"].clone()).unwrap();
    brief.inputs.sources.clear();
    brief.capabilities.sources.clear();
    brief.scope.source_versions.clear();
    // The submitting client and budget cover the project; each job narrows to its own task.
    brief.scope.task_id = None;
    let auth = Authority {
        tenant_id: Uuid::now_v7(),
        actor_id: Uuid::now_v7(),
        scope: brief.scope.clone(),
    };
    (auth, brief, Utc::now() + Duration::minutes(10))
}

/// Creates the policy and budget for an authority/brief pair; the brief's budget ID is made unique.
async fn world_in(
    store: Store,
    auth: Authority,
    mut brief: WorkBrief,
    deadline: DateTime<Utc>,
) -> World {
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
    World {
        store,
        auth,
        brief,
        deadline,
    }
}

impl World {
    async fn submit(&self, process: Process) -> Uuid {
        let mut brief = self.brief.clone();
        brief.process = process;
        // Each job is its own task; jobs sharing a task share one Pi session and lease.
        let task = Uuid::now_v7();
        brief.task_id = task;
        brief.scope.task_id = Some(task);
        let scope = brief.scope.clone();
        let job = self
            .store
            .submit_job(
                &self.auth,
                Command {
                    request_id: Uuid::now_v7(),
                    command_version: WireVersion::V1,
                    tenant_id: self.auth.tenant_id,
                    actor_id: self.auth.actor_id,
                    scope,
                    job_id: None,
                    session_id: None,
                    operation_id: None,
                    lane_id: None,
                    invocation_id: None,
                    expected_revisions: vec![],
                    deadline: self.deadline,
                    budget_id: brief.limits.root_budget_id,
                    lease_epoch: None,
                    payload: SubmitJob {
                        brief,
                        parent_id: None,
                        max_attempts: 2,
                        retain_until: self.deadline + Duration::days(1),
                    },
                },
            )
            .await
            .unwrap();
        job.id
    }
}

#[tokio::test]
async fn pools_claim_the_next_job_in_their_classes_and_receive_a_worker_credential() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::connect(&format!(
        "sqlite://{}?mode=rwc",
        temp.path().join("host.sqlite").display()
    ))
    .await
    .unwrap();
    let w = world(store.clone()).await;
    let credential = |name: &str, role: Role| Credential {
        token: name.repeat(32),
        role,
        authority: Authority {
            scope: Scope::default(),
            ..w.auth.clone()
        },
        expires_at: Utc::now() + Duration::hours(1),
    };
    let host = Host::new(
        store.clone(),
        vec![
            credential(
                "i",
                Role::Pool {
                    classes: vec![WorkClass::Interactive],
                },
            ),
            credential(
                "d",
                Role::Pool {
                    classes: vec![WorkClass::Deferred],
                },
            ),
            credential("c", Role::Client),
        ],
        temp.path().join("artifacts"),
    )
    .await
    .unwrap();
    let bearer = |name: &str| format!("Bearer {}", name.repeat(32));
    let claim = |processes: Vec<Process>| HostRequest::ClaimNext {
        processes,
        lease_seconds: 60,
    };

    // Nothing queued: an explicit idle reply, not an error.
    assert!(matches!(
        host.handle(Some(&bearer("i")), claim(vec![Process::Investigation]))
            .await
            .unwrap(),
        HostResponse::Idle
    ));
    // Pools only claim; clients only submit.
    assert_eq!(
        host.handle(Some(&bearer("c")), claim(vec![Process::Investigation]))
            .await
            .unwrap_err()
            .code,
        ReasonCode::ForbiddenScope
    );
    assert_eq!(
        host.handle(
            Some(&bearer("i")),
            HostRequest::InspectJob {
                job_id: Uuid::now_v7()
            }
        )
        .await
        .unwrap_err()
        .code,
        ReasonCode::ForbiddenScope
    );
    // A pool cannot ask for processes outside its classes.
    assert_eq!(
        host.handle(Some(&bearer("i")), claim(vec![Process::Formation]))
            .await
            .unwrap_err()
            .code,
        ReasonCode::ForbiddenScope
    );

    let formation = w.submit(Process::Formation).await;
    let first = w.submit(Process::Investigation).await;
    let second = w.submit(Process::Investigation).await;

    // The interactive pool sees investigations oldest first and never the formation job.
    let HostResponse::Work {
        assignment,
        credential: issued,
    } = host
        .handle(Some(&bearer("i")), claim(vec![Process::Investigation]))
        .await
        .unwrap()
    else {
        panic!("expected work");
    };
    assert_eq!(assignment.job.id, first);
    assert_eq!(issued.job_id, first);
    assert_eq!(
        assignment.owner_id, issued.actor_id,
        "lease owner is the issued worker actor"
    );
    // The worker credential drives its job.
    let worker = format!("Bearer {}", issued.token);
    let fence = memory_domain::coordination::Fence {
        job_id: first,
        owner_id: assignment.owner_id,
        epoch: assignment.epoch,
    };
    assert!(matches!(
        host.handle(Some(&worker), HostRequest::Start { fence })
            .await
            .unwrap(),
        HostResponse::Job { .. }
    ));
    // The pool only claims processes it names, and skips leased jobs.
    let HostResponse::Work { assignment, .. } = host
        .handle(
            Some(&bearer("i")),
            claim(vec![Process::Investigation, Process::Activation]),
        )
        .await
        .unwrap()
    else {
        panic!("expected work");
    };
    assert_eq!(assignment.job.id, second);
    assert!(matches!(
        host.handle(Some(&bearer("i")), claim(vec![Process::Investigation]))
            .await
            .unwrap(),
        HostResponse::Idle
    ));
    // The deferred pool gets the formation job; a pool that supports no deferred process stays idle.
    assert!(matches!(
        host.handle(Some(&bearer("d")), claim(vec![Process::Consolidation]))
            .await
            .unwrap(),
        HostResponse::Idle
    ));
    let HostResponse::Work { assignment, .. } = host
        .handle(
            Some(&bearer("d")),
            claim(vec![Process::Formation, Process::Consolidation]),
        )
        .await
        .unwrap()
    else {
        panic!("expected work");
    };
    assert_eq!(assignment.job.id, formation);
}

#[tokio::test]
async fn claiming_is_fair_across_projects_within_the_pool_scope() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::connect(&format!(
        "sqlite://{}?mode=rwc",
        temp.path().join("host.sqlite").display()
    ))
    .await
    .unwrap();
    let a = world(store.clone()).await;
    // A second project in the same tenant with its own budget and policy.
    let mut b = world(store.clone()).await;
    b.auth.tenant_id = a.auth.tenant_id;
    let b_project = Uuid::now_v7();
    b.auth.scope.project_id = Some(b_project);
    b.brief.scope.project_id = Some(b_project);
    let b = world_in(store.clone(), b.auth.clone(), b.brief.clone(), b.deadline).await;

    let busy = [
        a.submit(Process::Investigation).await,
        a.submit(Process::Investigation).await,
        a.submit(Process::Investigation).await,
    ];
    let quiet = b.submit(Process::Investigation).await;
    let host = Host::new(
        store.clone(),
        vec![Credential {
            token: "p".repeat(32),
            role: Role::Pool {
                classes: vec![WorkClass::Interactive],
            },
            authority: Authority {
                scope: Scope::default(),
                ..a.auth.clone()
            },
            expires_at: Utc::now() + Duration::hours(1),
        }],
        temp.path().join("artifacts"),
    )
    .await
    .unwrap();
    let mut order = Vec::new();
    for _ in 0..4 {
        let HostResponse::Work { assignment, .. } = host
            .handle(
                Some(&format!("Bearer {}", "p".repeat(32))),
                HostRequest::ClaimNext {
                    processes: vec![Process::Investigation],
                    lease_seconds: 60,
                },
            )
            .await
            .unwrap()
        else {
            panic!("expected work");
        };
        order.push(assignment.job.id);
    }
    // Oldest first when nothing is active, then the project with fewer active jobs.
    assert_eq!(order[0], busy[0]);
    assert_eq!(
        order[1], quiet,
        "the quiet project is served before the busy project's second job"
    );
    assert_eq!(&order[2..], &busy[1..]);
}
