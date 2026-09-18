//! ABOUTME: Tests for runtime worker credential issuance by an administrator.
//! ABOUTME: Issued credentials are bound to one job, its scope and its deadline.
use chrono::{Duration, Utc};
use memory_domain::{
    contracts::{Authority, Command, ReasonCode, Scope, WireVersion, WorkBrief},
    coordination::{Budget, HostRequest, HostResponse, Resources, SubmitJob},
};
use memory_host::{Credential, Host, Role};
use memory_store::Store;
use uuid::Uuid;

#[tokio::test]
async fn administrators_issue_job_bound_worker_credentials_at_runtime() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::connect(&format!(
        "sqlite://{}?mode=rwc",
        temp.path().join("host.sqlite").display()
    ))
    .await
    .unwrap();
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../evals/property-location/inputs/before-correction.json"
    ))
    .unwrap();
    let mut brief: WorkBrief =
        serde_json::from_value(fixture["command"]["payload"].clone()).unwrap();
    brief.inputs.sources.clear();
    brief.capabilities.sources.clear();
    brief.scope.source_versions.clear();
    let auth = Authority {
        tenant_id: Uuid::now_v7(),
        actor_id: Uuid::now_v7(),
        scope: brief.scope.clone(),
    };
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
    let deadline = Utc::now() + Duration::minutes(10);
    store
        .create_budget(
            &auth,
            Budget {
                id: brief.limits.root_budget_id,
                scope: auth.scope.clone(),
                limit: Resources {
                    tokens: 100_000,
                    provider_calls: 5,
                    output_bytes: 100_000,
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
                    max_attempts: 2,
                    retain_until: deadline + Duration::days(1),
                },
            },
        )
        .await
        .unwrap();

    let bearer = |name: &str| format!("Bearer {}", name.repeat(32));
    let credential = |name: &str, role: Role, scope: Scope| Credential {
        token: name.repeat(32),
        role,
        authority: Authority {
            scope,
            ..auth.clone()
        },
        expires_at: Utc::now() + Duration::hours(1),
    };
    let tenant_wide = Scope::default();
    let host = Host::new(
        store.clone(),
        vec![
            credential("a", Role::Administrator, tenant_wide.clone()),
            credential("c", Role::Client, auth.scope.clone()),
            credential(
                "n",
                Role::Administrator,
                Scope {
                    project_id: Some(Uuid::now_v7()),
                    ..Default::default()
                },
            ),
        ],
        temp.path().join("artifacts"),
    )
    .await
    .unwrap();

    // Clients cannot mint worker credentials.
    assert_eq!(
        host.handle(
            Some(&bearer("c")),
            HostRequest::IssueWorkerCredential { job_id: job.id }
        )
        .await
        .unwrap_err()
        .code,
        ReasonCode::ForbiddenScope
    );
    // An administrator for another project cannot see the job.
    assert_eq!(
        host.handle(
            Some(&bearer("n")),
            HostRequest::IssueWorkerCredential { job_id: job.id }
        )
        .await
        .unwrap_err()
        .code,
        ReasonCode::SourceUnavailable
    );
    // Unknown jobs are refused.
    assert!(
        host.handle(
            Some(&bearer("a")),
            HostRequest::IssueWorkerCredential {
                job_id: Uuid::now_v7()
            }
        )
        .await
        .is_err()
    );

    let issued = host
        .handle(
            Some(&bearer("a")),
            HostRequest::IssueWorkerCredential { job_id: job.id },
        )
        .await
        .unwrap();
    let HostResponse::WorkerCredential { credential } = issued else {
        panic!("expected a worker credential");
    };
    assert_eq!(credential.job_id, job.id);
    assert!(credential.token.len() >= 32);
    assert!(credential.expires_at <= deadline);
    assert_eq!(credential.scope, job.spec.brief.scope);

    // The issued token works only for its own job.
    let worker = format!("Bearer {}", credential.token);
    assert!(matches!(
        host.handle(Some(&worker), HostRequest::InspectJob { job_id: job.id })
            .await
            .unwrap(),
        HostResponse::Job { .. }
    ));
    assert_eq!(
        host.handle(
            Some(&worker),
            HostRequest::InspectJob {
                job_id: Uuid::now_v7()
            }
        )
        .await
        .unwrap_err()
        .code,
        ReasonCode::ForbiddenScope
    );
    // A worker cannot mint further credentials, and each issuance is a distinct token.
    assert_eq!(
        host.handle(
            Some(&worker),
            HostRequest::IssueWorkerCredential { job_id: job.id }
        )
        .await
        .unwrap_err()
        .code,
        ReasonCode::ForbiddenScope
    );
    let HostResponse::WorkerCredential { credential: second } = host
        .handle(
            Some(&bearer("a")),
            HostRequest::IssueWorkerCredential { job_id: job.id },
        )
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_ne!(second.token, credential.token);
    let HostResponse::Assignment { assignment } = host
        .handle(
            Some(&format!("Bearer {}", second.token)),
            HostRequest::Claim {
                job_id: job.id,
                lease_seconds: 60,
            },
        )
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(assignment.job.id, job.id);
}
