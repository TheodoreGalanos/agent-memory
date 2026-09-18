use chrono::{Duration, Utc};
use memory_domain::{contracts::*, coordination::*, interaction::*, records::*};
use memory_store::{Error, Store};
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
fn permit(assignment: &Assignment) -> Fence {
    Fence {
        job_id: assignment.job.id,
        owner_id: assignment.owner_id,
        epoch: assignment.epoch,
    }
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
async fn mutate(
    store: &Store,
    a: &Authority,
    m: UserMutation,
) -> memory_store::Result<UserResponse> {
    store
        .user_request(
            a,
            UserRequest::Mutate {
                request_id: Uuid::now_v7(),
                mutation: Box::new(m),
            },
        )
        .await
}
fn memory(r: UserResponse) -> MemoryVersion {
    let UserResponse::Memory { memory, .. } = r else {
        panic!("Expected memory")
    };
    *memory
}
fn exploration(r: UserResponse) -> Exploration {
    let UserResponse::Exploration { exploration } = r else {
        panic!("Expected exploration")
    };
    *exploration
}
async fn exercise(store: &Store) {
    let (a, command) = setup(store).await;
    let draft = record(&a, &command.payload.brief.policy);
    let request = UserRequest::Mutate {
        request_id: Uuid::now_v7(),
        mutation: Box::new(UserMutation::Contribute {
            record: Box::new(draft.clone()),
        }),
    };
    let initial = memory(store.user_request(&a, request.clone()).await.unwrap());
    assert!(matches!(
        initial.record.evidential_status,
        EvidentialStatus::AttributedStatement
    ));
    let retry = memory(store.user_request(&a, request).await.unwrap());
    assert_eq!(initial.reference, retry.reference);
    let mut correction = draft.clone();
    correction.content = MemoryContent::Knowledge {
        statement: "Corrected property value".into(),
        subject: None,
        predicate: None,
        uncertainty: vec![],
        examined_coverage: vec![],
    };
    let corrected = memory(
        mutate(
            store,
            &a,
            UserMutation::Correct {
                expected: initial.reference.clone(),
                record: Box::new(correction.clone()),
            },
        )
        .await
        .unwrap(),
    );
    assert_eq!(corrected.reference.revision.get(), 2);
    assert!(matches!(
        mutate(
            store,
            &a,
            UserMutation::Correct {
                expected: initial.reference.clone(),
                record: Box::new(correction)
            }
        )
        .await,
        Err(Error::Conflict)
    ));
    let UserResponse::History { records, .. } = store
        .user_request(
            &a,
            UserRequest::History {
                memory_id: initial.reference.memory_id,
                after_revision: 0,
                limit: 10,
            },
        )
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(records.len(), 2);
    let other = Authority {
        scope: Scope {
            project_id: Some(Uuid::now_v7()),
            ..a.scope.clone()
        },
        ..a.clone()
    };
    assert!(
        store
            .user_request(
                &other,
                UserRequest::InspectMemory {
                    reference: initial.reference.clone()
                }
            )
            .await
            .is_err()
    );
    assert!(
        mutate(
            store,
            &other,
            UserMutation::Correct {
                expected: corrected.reference,
                record: Box::new(draft.clone())
            }
        )
        .await
        .is_err()
    );
    let branch = exploration(
        mutate(
            store,
            &a,
            UserMutation::OpenExploration {
                scope: a.scope.clone(),
                purpose: "Hypothesis".into(),
                expires_at: Utc::now() + Duration::hours(1),
            },
        )
        .await
        .unwrap(),
    );
    let mut speculative = draft.clone();
    speculative.derived_from = vec![initial.reference.clone()];
    let branch = exploration(
        mutate(
            store,
            &a,
            UserMutation::AddExploration {
                id: branch.id,
                expected_revision: 1,
                record: Box::new(speculative),
            },
        )
        .await
        .unwrap(),
    );
    assert_eq!(
        store
            .memories(&a, &MemoryQuery::default())
            .await
            .unwrap()
            .len(),
        1
    );
    let promoted = memory(
        mutate(
            store,
            &a,
            UserMutation::PromoteExploration {
                id: branch.id,
                expected_revision: branch.revision,
                index: 0,
            },
        )
        .await
        .unwrap(),
    );
    assert!(matches!(
        promoted.record.evidential_status,
        EvidentialStatus::Assumption
    ));
    assert!(
        mutate(
            store,
            &a,
            UserMutation::PromoteExploration {
                id: branch.id,
                expected_revision: branch.revision + 1,
                index: 0
            }
        )
        .await
        .is_err()
    );
    let job = store.submit_job(&a, command.clone()).await.unwrap();
    let UserResponse::Task {
        manifests,
        coverage,
        ..
    } = store
        .user_request(
            &a,
            UserRequest::InspectTask {
                job_id: job.id,
                after_manifest: None,
            },
        )
        .await
        .unwrap()
    else {
        panic!()
    };
    assert!(manifests.is_empty());
    assert!(coverage.unexamined[0].contains("No render"));
    let UserResponse::Decisions { decisions } = mutate(
        store,
        &a,
        UserMutation::RequestDecision {
            fence: None,
            job_id: job.id,
            owner_id: a.actor_id,
            question: "Proceed?".into(),
            missing: "Owner authority".into(),
            deadline: Utc::now() + Duration::minutes(10),
        },
    )
    .await
    .unwrap() else {
        panic!()
    };
    let decision = &decisions[0];
    let assignment = store.claim_job(&a, job.id, 120).await.unwrap();
    let fence = permit(&assignment);
    assert!(store.start_job(&a, &fence).await.is_err());
    assert!(
        store
            .check_execution(&a, &fence, Some("fixture"), None)
            .await
            .is_err()
    );
    // Muting delivery is not a decision and cannot authorize work.
    mutate(
        store,
        &a,
        UserMutation::NotificationPreference {
            mode: NotificationMode::Muted,
        },
    )
    .await
    .unwrap();
    let UserResponse::Notifications { page, .. } = store
        .user_request(
            &a,
            UserRequest::Notifications {
                after: 0,
                limit: 100,
            },
        )
        .await
        .unwrap()
    else {
        panic!()
    };
    assert!(page.events.is_empty());
    assert!(store.start_job(&a, &fence).await.is_err());
    let nonowner = Authority {
        actor_id: Uuid::now_v7(),
        ..a.clone()
    };
    assert!(matches!(
        mutate(
            store,
            &nonowner,
            UserMutation::AnswerDecision {
                id: decision.id,
                expected_revision: 1,
                answer: DecisionAnswer::Approve,
                reason: "Wrong actor".into()
            }
        )
        .await,
        Err(Error::Forbidden)
    ));
    let mut independent = command.clone();
    independent.request_id = Uuid::now_v7();
    independent.payload.brief.profile.id = Uuid::now_v7();
    let broad = Authority {
        scope: Scope {
            task_id: None,
            ..a.scope.clone()
        },
        ..a.clone()
    };
    let unrelated = store.submit_job(&broad, independent).await.unwrap();
    let unrelated_assignment = store.claim_job(&broad, unrelated.id, 120).await.unwrap();
    store
        .start_job(&broad, &permit(&unrelated_assignment))
        .await
        .unwrap();
    mutate(
        store,
        &a,
        UserMutation::AnswerDecision {
            id: decision.id,
            expected_revision: 1,
            answer: DecisionAnswer::Approve,
            reason: "Explicit approval".into(),
        },
    )
    .await
    .unwrap();
    store.start_job(&a, &fence).await.unwrap();
    let notices = store.memory_changes(&a, 0, 100).await.unwrap();
    assert_eq!(notices.changes.len(), 1);
    assert_eq!(notices.changes[0].previous, initial.reference);
    // Another job can still execute while the first job awaits its owner.

    let mut manifest:memory_domain::workspace::RenderManifest=serde_json::from_value(serde_json::json!({
      "schema_version":"1","decision_id":Uuid::now_v7(),"workspace_id":Uuid::now_v7(),"session_id":job.session_id,"lane":"main","operation_id":job.operation_id,"profile":job.spec.brief.profile,"provider":"fixture","model":"scripted","access_revision":"test","selected":[],"sources":[],"conflicts":[],"deferred":[],"next_step":null,"messages":[{"role":"user","content":"Recorded test context"}],"tool_schemas":[],"strategy":"test","rebuild_reasons":[],"estimator":"test","estimated_input_tokens":10,"reserved_output_tokens":100,"reserved_result_tokens":100,"final_payload_bytes":100,"status":"rendered","usage":{}
    })).unwrap();
    store
        .record_context(&a, &fence, manifest.clone())
        .await
        .unwrap();
    manifest.status = "responded".into();
    store
        .record_context(&a, &fence, manifest.clone())
        .await
        .unwrap();
    for _ in 0..3 {
        manifest.decision_id = Uuid::now_v7();
        store
            .record_context(&a, &fence, manifest.clone())
            .await
            .unwrap();
    }
    let UserResponse::Task {
        manifests,
        next_manifest,
        ..
    } = store
        .user_request(
            &a,
            UserRequest::InspectTask {
                job_id: job.id,
                after_manifest: None,
            },
        )
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(manifests.len(), 3);
    assert_eq!(manifests[0].status, "responded");
    let UserResponse::Task {
        manifests,
        next_manifest: next,
        ..
    } = store
        .user_request(
            &a,
            UserRequest::InspectTask {
                job_id: job.id,
                after_manifest: next_manifest,
            },
        )
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(manifests.len(), 1);
    assert!(next.is_none());
    manifest.session_id = "another-session".into();
    assert!(matches!(
        store.record_context(&a, &fence, manifest).await,
        Err(Error::Forbidden)
    ));

    mutate(
        store,
        &a,
        UserMutation::NotificationPreference {
            mode: NotificationMode::Material,
        },
    )
    .await
    .unwrap();
    let UserResponse::Notifications { page, .. } = store
        .user_request(
            &a,
            UserRequest::Notifications {
                after: 0,
                limit: 100,
            },
        )
        .await
        .unwrap()
    else {
        panic!()
    };
    let event = page.events[0].id;
    mutate(
        store,
        &a,
        UserMutation::AcknowledgeNotification { event_id: event },
    )
    .await
    .unwrap();
    let UserResponse::Notifications { page, .. } = store
        .user_request(
            &a,
            UserRequest::Notifications {
                after: 0,
                limit: 100,
            },
        )
        .await
        .unwrap()
    else {
        panic!()
    };
    assert!(!page.events.iter().any(|e| e.id == event));
    let admin = Authority {
        scope: Scope::default(),
        ..a.clone()
    };
    mutate(
        store,
        &admin,
        UserMutation::SetControl {
            kind: ControlKind::Provider,
            target: "fixture".into(),
            expected_revision: 0,
            enabled: false,
        },
    )
    .await
    .unwrap();
    assert!(
        store
            .check_execution(&a, &fence, Some("fixture"), None)
            .await
            .is_err()
    );
    mutate(
        store,
        &admin,
        UserMutation::SetControl {
            kind: ControlKind::Provider,
            target: "fixture".into(),
            expected_revision: 1,
            enabled: true,
        },
    )
    .await
    .unwrap();
    store
        .check_execution(&a, &fence, Some("fixture"), None)
        .await
        .unwrap();
    for (kind, target) in [
        (ControlKind::Family, "J16".to_string()),
        (ControlKind::Profile, job.spec.brief.profile.id.to_string()),
    ] {
        mutate(
            store,
            &admin,
            UserMutation::SetControl {
                kind,
                target: target.clone(),
                expected_revision: 0,
                enabled: false,
            },
        )
        .await
        .unwrap();
        assert!(
            store
                .check_execution(&a, &fence, None, Some("J16"))
                .await
                .is_err()
        );
        mutate(
            store,
            &admin,
            UserMutation::SetControl {
                kind,
                target,
                expected_revision: 1,
                enabled: true,
            },
        )
        .await
        .unwrap();
    }
    let UserResponse::Decisions { decisions } = mutate(
        store,
        &a,
        UserMutation::RequestDecision {
            fence: Some(fence.clone()),
            job_id: job.id,
            owner_id: a.actor_id,
            question: "Another decision".into(),
            missing: "Fresh authority".into(),
            deadline: Utc::now() + Duration::milliseconds(30),
        },
    )
    .await
    .unwrap() else {
        panic!()
    };
    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    assert!(store.check_execution(&a, &fence, None, None).await.is_err());
    assert!(matches!(
        mutate(
            store,
            &a,
            UserMutation::AnswerDecision {
                id: decisions[0].id,
                expected_revision: 1,
                answer: DecisionAnswer::Approve,
                reason: "Too late".into()
            }
        )
        .await,
        Err(Error::Conflict)
    ));
    // Deleting an input denies the temporary copy before physical cleanup, and purges it later.
    let report = store
        .begin_deletion(
            &a,
            memory_domain::retention::DeletionRequest {
                id: Uuid::now_v7(),
                resources: vec![initial.reference.memory_id],
            },
        )
        .await
        .unwrap();
    assert!(report.resources.contains(&branch.id));
    assert!(
        store
            .user_request(&a, UserRequest::InspectExploration { id: branch.id })
            .await
            .is_err()
    );
    let dir = tempfile::tempdir().unwrap();
    let artifacts = memory_store::artifacts::ArtifactService::local(store.clone(), dir.path())
        .await
        .unwrap();
    artifacts.purge_deletion(&a, report.id).await.unwrap();
}
#[tokio::test]
async fn sqlite_interaction() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::connect(&format!(
        "sqlite://{}?mode=rwc",
        dir.path().join("test.db").display()
    ))
    .await
    .unwrap();
    exercise(&store).await;
}
#[tokio::test]
#[ignore = "requires MEMORY_TEST_POSTGRES_URL"]
async fn postgres_interaction() {
    let store = Store::connect(&std::env::var("MEMORY_TEST_POSTGRES_URL").unwrap())
        .await
        .unwrap();
    exercise(&store).await;
}
