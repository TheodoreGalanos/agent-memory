use chrono::{Duration, Utc};
use memory_domain::{
    contracts::{Authority, ReasonCode, Scope},
    coordination::{Budget, Fence, HostRequest, HostResponse, Resources},
};
use memory_host::{Credential, Host, Role};
use memory_store::Store;
use uuid::Uuid;

#[tokio::test]
async fn credentials_bound_tenant_role_assignment_and_expiry() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::connect(&format!(
        "sqlite://{}?mode=rwc",
        temp.path().join("host.sqlite").display()
    ))
    .await
    .unwrap();
    let auth = Authority {
        tenant_id: Uuid::now_v7(),
        actor_id: Uuid::now_v7(),
        scope: Scope {
            project_id: Some(Uuid::now_v7()),
            ..Default::default()
        },
    };
    let credential = |name: &str, role: Role, authority: Authority, expired: bool| Credential {
        token: name.repeat(32),
        role,
        authority,
        expires_at: if expired {
            Utc::now() - Duration::seconds(1)
        } else {
            Utc::now() + Duration::hours(1)
        },
    };
    let assigned_job = Uuid::now_v7();
    let host = Host::new(
        store.clone(),
        vec![
            credential("a", Role::Administrator, auth.clone(), false),
            credential("c", Role::Client, auth.clone(), false),
            credential(
                "w",
                Role::Worker {
                    job_id: assigned_job,
                },
                auth.clone(),
                false,
            ),
            credential("x", Role::Administrator, auth.clone(), true),
            credential(
                "t",
                Role::Client,
                Authority {
                    tenant_id: Uuid::now_v7(),
                    ..auth.clone()
                },
                false,
            ),
        ],
        temp.path().join("artifacts"),
    )
    .await
    .unwrap();
    let inspect = || HostRequest::InspectJob {
        job_id: Uuid::now_v7(),
    };
    assert_eq!(
        host.handle(None, inspect()).await.unwrap_err().code,
        ReasonCode::Unauthenticated
    );
    assert_eq!(
        host.handle(Some("Bearer unknown"), inspect())
            .await
            .unwrap_err()
            .code,
        ReasonCode::Unauthenticated
    );
    assert_eq!(
        host.handle(Some(&format!("Bearer {}", "x".repeat(32))), inspect())
            .await
            .unwrap_err()
            .code,
        ReasonCode::Unauthenticated
    );
    assert_eq!(
        host.handle(Some(&format!("Bearer {}", "w".repeat(32))), inspect())
            .await
            .unwrap_err()
            .code,
        ReasonCode::ForbiddenScope
    );
    assert_eq!(
        host.handle(
            Some(&format!("Bearer {}", "c".repeat(32))),
            HostRequest::Recover
        )
        .await
        .unwrap_err()
        .code,
        ReasonCode::ForbiddenScope
    );
    assert_eq!(
        host.handle(
            Some(&format!("Bearer {}", "w".repeat(32))),
            HostRequest::Reserve {
                fence: Fence {
                    job_id: assigned_job,
                    owner_id: auth.actor_id,
                    epoch: 1
                },
                provider_attempt: "cannot-use-final-reserve".into(),
                maximum: Resources::default(),
                final_result: true,
            }
        )
        .await
        .unwrap_err()
        .code,
        ReasonCode::ForbiddenScope
    );
    let budget = Budget {
        id: Uuid::now_v7(),
        scope: auth.scope.clone(),
        limit: Resources {
            tokens: 100,
            ..Default::default()
        },
        final_result_reserve: Resources {
            tokens: 10,
            ..Default::default()
        },
        deadline: Utc::now() + Duration::minutes(10),
        max_child_depth: 2,
        max_child_concurrency: 1,
        pricing_revision: "test".into(),
    };
    let response = host
        .handle(
            Some(&format!("Bearer {}", "a".repeat(32))),
            HostRequest::CreateBudget {
                budget: Box::new(budget.clone()),
            },
        )
        .await
        .unwrap();
    assert!(matches!(response, HostResponse::Budget { .. }));
    assert!(matches!(
        host.handle(
            Some(&format!("Bearer {}", "c".repeat(32))),
            HostRequest::BudgetUsage {
                budget_id: budget.id
            }
        )
        .await
        .unwrap(),
        HostResponse::BudgetUsage { .. }
    ));
    assert_eq!(
        host.handle(
            Some(&format!("Bearer {}", "t".repeat(32))),
            HostRequest::BudgetUsage {
                budget_id: budget.id
            }
        )
        .await
        .unwrap_err()
        .code,
        ReasonCode::SourceUnavailable
    );
    store.close().await;
}
