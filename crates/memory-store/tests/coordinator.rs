use chrono::{Duration, Utc};
use memory_domain::{contracts::*, coordination::*, records::*};
use memory_store::{Error, Store, coordinator::MemoryCommit};
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
                    tokens: 100,
                    provider_calls: 4,
                    output_bytes: 1024,
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

async fn durable_jobs(store: &Store) {
    let (auth, command) = setup(store).await;
    let job = store.submit_job(&auth, command.clone()).await.unwrap();
    assert_eq!(
        store.submit_job(&auth, command.clone()).await.unwrap().id,
        job.id
    );
    let mut changed = command.clone();
    changed.payload.brief.purpose = "Different work".into();
    assert!(matches!(
        store.submit_job(&auth, changed).await,
        Err(Error::Conflict)
    ));
    let events = store.events(&auth, 0, 100).await.unwrap();
    assert_eq!(events.events.len(), 1);
    let assignment = store.claim_job(&auth, job.id, 30).await.unwrap();
    assert!(matches!(
        store.claim_job(&auth, job.id, 30).await,
        Err(Error::Conflict)
    ));
    let permit = permit(&assignment);
    store.start_job(&auth, &permit).await.unwrap();
    let commit = Command {
        request_id: Uuid::now_v7(),
        command_version: WireVersion::V1,
        tenant_id: auth.tenant_id,
        actor_id: auth.actor_id,
        scope: auth.scope.clone(),
        job_id: Some(job.id),
        session_id: Some(job.session_id.clone()),
        lane_id: Some("main".into()),
        operation_id: Some(job.operation_id.clone()),
        invocation_id: Some("contribute-1".into()),
        expected_revisions: vec![],
        deadline: command.deadline,
        budget_id: command.budget_id,
        lease_epoch: Some(permit.epoch.try_into().unwrap()),
        payload: MemoryCommit {
            fence: permit.clone(),
            changes: vec![MemoryChange::Create(record(
                &auth,
                &command.payload.brief.policy,
            ))],
        },
    };
    let first = store.commit_memories(&auth, commit.clone()).await.unwrap();
    let replay = store.commit_memories(&auth, commit.clone()).await.unwrap();
    assert_eq!(first[0].reference, replay[0].reference);
    assert_eq!(
        store
            .memories(&auth, &MemoryQuery::default())
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store
            .events(&auth, events.cursor, 100)
            .await
            .unwrap()
            .events
            .iter()
            .filter(|e| e.kind == "memory_changed")
            .count(),
        1
    );
    let mut invalid = commit.clone();
    invalid.request_id = Uuid::now_v7();
    let mut empty = record(&auth, &command.payload.brief.policy);
    empty.label.clear();
    invalid.payload.changes.push(MemoryChange::Create(empty));
    assert!(matches!(
        store.commit_memories(&auth, invalid).await,
        Err(Error::Invalid(_))
    ));
    assert_eq!(
        store
            .memories(&auth, &MemoryQuery::default())
            .await
            .unwrap()
            .len(),
        1
    );
    let done_id = Uuid::now_v7();
    let done = store
        .complete_job(&auth, done_id, &permit, result(&auth))
        .await
        .unwrap();
    assert_eq!(done.state, JobState::Completed);
    assert_eq!(
        store
            .complete_job(&auth, done_id, &permit, result(&auth))
            .await
            .unwrap()
            .id,
        job.id
    );
    assert!(matches!(
        store.start_job(&auth, &permit).await,
        Err(Error::Conflict)
    ));
    assert_eq!(
        store.commit_memories(&auth, commit.clone()).await.unwrap()[0].reference,
        first[0].reference
    );
    let replacement = Authority {
        actor_id: Uuid::now_v7(),
        ..auth.clone()
    };
    let mut replay = commit.clone();
    replay.actor_id = replacement.actor_id;
    replay.lease_epoch = Some((permit.epoch + 1).try_into().unwrap());
    replay.payload.fence.owner_id = replacement.actor_id;
    replay.payload.fence.epoch += 1;
    assert_eq!(
        store.commit_memories(&replacement, replay).await.unwrap()[0].reference,
        first[0].reference
    );
    let replacement_fence = Fence {
        owner_id: replacement.actor_id,
        epoch: permit.epoch + 1,
        ..permit.clone()
    };
    assert_eq!(
        store
            .complete_job(&replacement, done_id, &replacement_fence, result(&auth))
            .await
            .unwrap()
            .id,
        job.id
    );
    let other = Authority {
        tenant_id: Uuid::now_v7(),
        actor_id: auth.actor_id,
        scope: auth.scope.clone(),
    };
    assert!(matches!(
        store.job(&other, job.id).await,
        Err(Error::NotFound)
    ));
    assert!(
        store
            .events(&other, 0, 100)
            .await
            .unwrap()
            .events
            .is_empty()
    );
    let mut delivered = command;
    delivered.request_id = Uuid::now_v7();
    let child = store
        .consume_event(&auth, "formation", events.events[0].id, delivered.clone())
        .await
        .unwrap();
    delivered.request_id = Uuid::now_v7();
    assert_eq!(
        store
            .consume_event(&auth, "formation", events.events[0].id, delivered)
            .await
            .unwrap()
            .id,
        child.id
    );
}

async fn budgets_and_children(store: &Store) {
    let (auth, command) = setup(store).await;
    let root = store.submit_job(&auth, command.clone()).await.unwrap();
    let assignment = store.claim_job(&auth, root.id, 30).await.unwrap();
    let root_permit = permit(&assignment);
    let mut child_command = command.clone();
    child_command.request_id = Uuid::now_v7();
    child_command.payload.parent_id = Some(root.id);
    let child = store
        .submit_job(&auth, child_command.clone())
        .await
        .unwrap();
    assert_ne!(child.session_id, root.session_id);
    assert_eq!(child.root_id, root.id);
    let child_assignment = store.claim_job(&auth, child.id, 30).await.unwrap();
    child_command.request_id = Uuid::now_v7();
    let sibling = store.submit_job(&auth, child_command).await.unwrap();
    assert!(matches!(
        store.claim_job(&auth, sibling.id, 30).await,
        Err(Error::LimitExceeded)
    ));
    let reserved = store
        .reserve_usage(
            &auth,
            &permit(&child_assignment),
            "provider-call-1",
            Resources {
                tokens: 80,
                provider_calls: 1,
                ..Default::default()
            },
            false,
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .reserve_usage(
                &auth,
                &permit(&child_assignment),
                "provider-call-1",
                reserved.maximum,
                false
            )
            .await
            .unwrap()
            .id,
        reserved.id
    );
    assert!(matches!(
        store
            .reserve_usage(
                &auth,
                &root_permit,
                "provider-call-2",
                Resources {
                    tokens: 20,
                    ..Default::default()
                },
                false
            )
            .await,
        Err(Error::LimitExceeded)
    ));
    store
        .settle_usage(&auth, &permit(&child_assignment), reserved.id, None)
        .await
        .unwrap();
    let usage = store.budget_usage(&auth, command.budget_id).await.unwrap();
    assert_eq!(usage.unresolved.tokens, 80);
    assert_eq!(usage.committed.tokens, 0);
    store
        .settle_usage(
            &auth,
            &permit(&child_assignment),
            reserved.id,
            Some(Resources {
                tokens: 60,
                provider_calls: 1,
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    store
        .settle_usage(
            &auth,
            &permit(&child_assignment),
            reserved.id,
            Some(Resources {
                tokens: 60,
                provider_calls: 1,
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .budget_usage(&auth, command.budget_id)
            .await
            .unwrap()
            .committed
            .tokens,
        60
    );
    assert!(matches!(
        store
            .settle_usage(
                &auth,
                &permit(&child_assignment),
                reserved.id,
                Some(Resources::default())
            )
            .await,
        Err(Error::Conflict)
    ));
    store
        .reserve_usage(
            &auth,
            &root_permit,
            "final-response",
            Resources {
                tokens: 40,
                ..Default::default()
            },
            true,
        )
        .await
        .unwrap();
    store.cancel_job(&auth, root.id).await.unwrap();
    assert!(store.job(&auth, child.id).await.unwrap().cancel_requested);
    assert!(store.job(&auth, sibling.id).await.unwrap().cancel_requested);
    assert!(matches!(
        store
            .reserve_usage(&auth, &root_permit, "too-late", Resources::default(), false)
            .await,
        Err(Error::Conflict)
    ));
}

async fn expired_owner_and_unknown_effect(store: &Store) {
    let (auth, command) = setup(store).await;
    let job = store.submit_job(&auth, command).await.unwrap();
    let first = store.claim_job(&auth, job.id, 1).await.unwrap();
    let request = EffectRequest {
        logical_operation_id: format!("{}/invocation-1", job.operation_id),
        kind: "shell/invocation-1".into(),
        invocation_id: "invocation-1".into(),
        replay: ReplayClass::Reconcile,
        arguments: serde_json::json!({"command":"create output"}),
    };
    let effect = store
        .prepare_effect(&auth, &permit(&first), request.clone())
        .await
        .unwrap();
    store
        .begin_effect(&auth, &permit(&first), effect.id)
        .await
        .unwrap();
    assert!(matches!(
        store.begin_effect(&auth, &permit(&first), effect.id).await,
        Err(Error::Conflict)
    ));
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    store.recover_jobs(&auth).await.unwrap();
    assert_eq!(
        store.job(&auth, job.id).await.unwrap().state,
        JobState::Waiting
    );
    assert_eq!(
        store.effect(&auth, effect.id).await.unwrap().state,
        EffectState::OutcomeUnknown
    );
    assert!(matches!(
        store.claim_job(&auth, job.id, 30).await,
        Err(Error::Conflict)
    ));
    assert!(matches!(
        store
            .report_effect(
                &auth,
                &permit(&first),
                effect.id,
                EffectState::Succeeded,
                serde_json::json!({"ok":true})
            )
            .await,
        Err(Error::Conflict)
    ));
    store
        .reconcile_effect(
            &auth,
            effect.id,
            EffectResolution::NotPerformed,
            serde_json::json!({"checked_remote_marker":"absent"}),
        )
        .await
        .unwrap();
    store.recover_jobs(&auth).await.unwrap();
    let second = store.claim_job(&auth, job.id, 30).await.unwrap();
    assert!(second.epoch > first.epoch);
    assert_eq!(second.job.operation_id, first.job.operation_id);
    assert!(matches!(
        store.start_job(&auth, &permit(&first)).await,
        Err(Error::Conflict)
    ));
    assert_eq!(
        store
            .prepare_effect(&auth, &permit(&second), request)
            .await
            .unwrap()
            .id,
        effect.id
    );
    store
        .begin_effect(&auth, &permit(&second), effect.id)
        .await
        .unwrap();
    store
        .report_effect(
            &auth,
            &permit(&second),
            effect.id,
            EffectState::Succeeded,
            serde_json::json!({"remote_receipt":"created-once"}),
        )
        .await
        .unwrap();
    store
        .complete_job(&auth, Uuid::now_v7(), &permit(&second), result(&auth))
        .await
        .unwrap();
}

async fn suite(url: &str) {
    let store = Store::connect(url).await.unwrap();
    durable_jobs(&store).await;
    scoped_children(&store).await;
    budgets_and_children(&store).await;
    expired_owner_and_unknown_effect(&store).await;
    timers_and_retention(&store, url).await;
    store.close().await;
}
#[tokio::test]
async fn sqlite_coordination() {
    let temp = tempfile::tempdir().unwrap();
    suite(&format!(
        "sqlite://{}?mode=rwc",
        temp.path().join("coord.sqlite").display()
    ))
    .await;
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL; run npm run test:store"]
async fn postgres_coordination() {
    suite(&std::env::var("MEMORY_TEST_POSTGRES_URL").unwrap()).await;
}

async fn timers_and_retention(store: &Store, url: &str) {
    let (auth, command) = setup(store).await;
    let job = store.submit_job(&auth, command.clone()).await.unwrap();
    let first = store.claim_job(&auth, job.id, 30).await.unwrap();
    store
        .wait_job(
            &auth,
            &permit(&first),
            "retry provider",
            Utc::now() + Duration::seconds(1),
        )
        .await
        .unwrap();
    assert!(matches!(
        store.claim_job(&auth, job.id, 30).await,
        Err(Error::Conflict)
    ));
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    store.recover_jobs(&auth).await.unwrap();
    let second = store.claim_job(&auth, job.id, 30).await.unwrap();
    assert_eq!(second.job.operation_id, job.operation_id);
    store.cancel_job(&auth, job.id).await.unwrap();
    assert_eq!(
        store
            .acknowledge_cancellation(&auth, &permit(&second))
            .await
            .unwrap()
            .state,
        JobState::Cancelled
    );
    // Advance retention timestamps in this disposable database without waiting a day.
    let pool = sqlx::AnyPool::connect(url).await.unwrap();
    sqlx::query("UPDATE jobs SET retain_until=0 WHERE tenant_id=$1")
        .bind(auth.tenant_id.to_string())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE outbox_events SET expires_at=0 WHERE tenant_id=$1")
        .bind(auth.tenant_id.to_string())
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let admin = Authority {
        tenant_id: auth.tenant_id,
        actor_id: auth.actor_id,
        scope: Scope::default(),
    };
    assert_eq!(store.prune_job_history(&admin).await.unwrap(), 1);
    assert!(matches!(
        store.job(&auth, job.id).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        store.submit_job(&auth, command).await,
        Err(Error::Unavailable)
    ));
    assert!(store.events(&auth, 0, 100).await.unwrap().snapshot_required);
}

async fn scoped_children(store: &Store) {
    let (auth, command) = setup(store).await;
    let parent = store.submit_job(&auth, command.clone()).await.unwrap();
    let assignment = store.claim_job(&auth, parent.id, 30).await.unwrap();
    let fence = permit(&assignment);
    let mut brief = command.payload.brief.clone();
    brief.purpose = "Inspect the counterexample".into();
    let request_id = Uuid::now_v7();
    let child = store
        .spawn_child(
            &auth,
            &fence,
            request_id,
            brief.clone(),
            command.deadline,
            None,
        )
        .await
        .unwrap();
    assert_ne!(child.session_id, parent.session_id);
    assert_eq!(
        store
            .spawn_child(
                &auth,
                &fence,
                request_id,
                brief.clone(),
                command.deadline,
                None
            )
            .await
            .unwrap()
            .id,
        child.id
    );
    assert!(matches!(
        store
            .spawn_child(
                &auth,
                &fence,
                Uuid::now_v7(),
                brief.clone(),
                command.deadline,
                None
            )
            .await,
        Err(Error::LimitExceeded)
    ));
    let mut unassigned = brief.clone();
    unassigned.inputs.artifacts.push(Uuid::now_v7());
    assert!(matches!(
        store
            .spawn_child(
                &auth,
                &fence,
                Uuid::now_v7(),
                unassigned,
                command.deadline,
                None
            )
            .await,
        Err(Error::Forbidden)
    ));
    let mut broader = brief.clone();
    broader.capabilities.tools.push("ungranted-tool".into());
    assert!(matches!(
        store
            .spawn_child(
                &auth,
                &fence,
                Uuid::now_v7(),
                broader,
                command.deadline,
                None
            )
            .await,
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        store.child_jobs(&auth, &fence, &[parent.id]).await,
        Err(Error::Forbidden)
    ));
    store
        .wait_children(
            &auth,
            &fence,
            &[child.id],
            Utc::now() + Duration::minutes(5),
        )
        .await
        .unwrap();
    store.recover_jobs(&auth).await.unwrap();
    assert!(matches!(
        store.claim_job(&auth, parent.id, 30).await,
        Err(Error::Conflict)
    ));
    let child_owner = store.claim_job(&auth, child.id, 30).await.unwrap();
    let root = tempfile::tempdir().unwrap();
    let artifacts = memory_store::artifacts::ArtifactService::local(store.clone(), root.path())
        .await
        .unwrap();
    let artifact = artifacts
        .allocate(
            &auth,
            memory_domain::sources::ArtifactSpec {
                label: "Child result".into(),
                scope: auth.scope.clone(),
                media_type: "application/json".into(),
                expected_bytes: 2,
                origin: Origin::AgentGenerated,
                retention_class: "test".into(),
                dependencies: vec![],
            },
        )
        .await
        .unwrap();
    artifacts
        .upload(&auth, artifact.id, artifact.revision, b"{}".as_slice())
        .await
        .unwrap();
    let mut completed = result(&auth);
    completed.result_artifact = Some(artifact.id);
    store
        .complete_job(&auth, Uuid::now_v7(), &permit(&child_owner), completed)
        .await
        .unwrap();
    store.recover_jobs(&auth).await.unwrap();
    let resumed = store.claim_job(&auth, parent.id, 30).await.unwrap();
    assert!(resumed.epoch > assignment.epoch);
    let resumed_fence = permit(&resumed);
    store
        .input_artifact(&auth, &resumed_fence, artifact.id)
        .await
        .unwrap();
    let reuse_request = Uuid::now_v7();
    let reuse_brief = brief.clone();
    let reused = store
        .spawn_child(
            &auth,
            &resumed_fence,
            reuse_request,
            brief.clone(),
            command.deadline,
            Some(child.id),
        )
        .await
        .unwrap();
    assert_eq!(reused.id, child.id);
    brief.definitions.push("Different interpretation".into());
    assert!(matches!(
        store
            .spawn_child(
                &auth,
                &resumed_fence,
                Uuid::now_v7(),
                brief,
                command.deadline,
                Some(child.id)
            )
            .await,
        Err(Error::Conflict)
    ));
    assert!(store.child_jobs(&auth, &fence, &[child.id]).await.is_err());
    let live_child = store
        .spawn_child(
            &auth,
            &resumed_fence,
            Uuid::now_v7(),
            reuse_brief.clone(),
            command.deadline,
            None,
        )
        .await
        .unwrap();
    // Revocation applies to cached-result receipts as well as new cache lookups.
    let ready = artifacts.inspect(&auth, artifact.id).await.unwrap();
    artifacts
        .revoke(&auth, artifact.id, ready.revision)
        .await
        .unwrap();
    assert!(
        store
            .child_jobs(&auth, &resumed_fence, &[child.id])
            .await
            .is_err()
    );
    assert!(
        store
            .spawn_child(
                &auth,
                &resumed_fence,
                reuse_request,
                reuse_brief.clone(),
                command.deadline,
                Some(child.id)
            )
            .await
            .is_err()
    );
    store.cancel_job(&auth, parent.id).await.unwrap();
    assert!(
        store
            .job(&auth, live_child.id)
            .await
            .unwrap()
            .cancel_requested
    );
    assert!(
        store
            .spawn_child(
                &auth,
                &resumed_fence,
                Uuid::now_v7(),
                reuse_brief,
                command.deadline,
                None
            )
            .await
            .is_err()
    );
}
