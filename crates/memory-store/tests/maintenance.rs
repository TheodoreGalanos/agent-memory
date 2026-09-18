use chrono::{Duration, Utc};
use memory_domain::{
    contracts::*, coordination::*, intentions::*, maintenance::*, records::*, sources::*,
};
use memory_store::{Error, Store, artifacts::ArtifactService, maintenance::MaintenanceDisposition};
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
async fn add_intention(store: &Store, a: &Authority, r: RecordDraft) -> IntentionOccurrence {
    let m = store.create_memory(a, r).await.unwrap();
    store
        .intentions(a, m.reference.memory_id)
        .await
        .unwrap()
        .remove(0)
}
async fn finish(
    store: &Store,
    service: &ArtifactService,
    a: &Authority,
    o: &IntentionOccurrence,
    status: WorkStatus,
) -> WorkResult {
    let assigned = store.claim_job(a, o.job_id.unwrap(), 120).await.unwrap();
    finish_assigned(store, service, a, &assigned, status).await
}
async fn finish_assigned(
    store: &Store,
    service: &ArtifactService,
    a: &Authority,
    assigned: &Assignment,
    status: WorkStatus,
) -> WorkResult {
    let mut r = result(a);
    r.status = status;
    if status != WorkStatus::Complete {
        r.unresolved_work.push("Verification incomplete".into());
    }
    let bytes = serde_json::to_vec(&r).unwrap();
    let artifact = service
        .allocate_named(
            a,
            Uuid::now_v7(),
            ArtifactSpec {
                label: "Checked result".into(),
                scope: a.scope.clone(),
                media_type: "application/json".into(),
                expected_bytes: bytes.len() as u32,
                origin: Origin::AgentGenerated,
                retention_class: "test".into(),
                dependencies: vec![],
            },
        )
        .await
        .unwrap();
    service
        .upload(a, artifact.id, artifact.revision, bytes.as_slice())
        .await
        .unwrap();
    r.result_artifact = Some(artifact.id);
    store
        .complete_job(a, Uuid::now_v7(), &permit(assigned), r.clone())
        .await
        .unwrap();
    r
}
async fn lifecycle(store: &Store, url: &str, service: &ArtifactService) {
    let (a, command) = setup(store).await;
    let r = definition(&a, &command.payload.brief);
    let o = add_intention(store, &a, r.clone()).await;
    assert_eq!(o.state, IntentionState::Pending);
    assert!(store.confirm_intention(&a, o.id).await.is_err());
    let armed = store.arm_intention(&a, o.id, true, None).await.unwrap();
    assert_eq!(armed.event_cursor, o.event_cursor);
    let (one, two) = tokio::join!(
        store.fire_intention(&a, o.id, None, false, None),
        store.fire_intention(&a, o.id, None, false, None)
    );
    let fired = one.unwrap();
    assert_eq!(fired.state, IntentionState::Fired);
    assert_eq!(two.unwrap().job_id, fired.job_id);
    assert!(
        store
            .revise_memory(&a, &o.definition.reference, r.clone())
            .await
            .is_err()
    );
    let first_attempt = store
        .claim_job(&a, fired.job_id.unwrap(), 120)
        .await
        .unwrap();
    store
        .wait_job(
            &a,
            &permit(&first_attempt),
            "Retry later",
            Utc::now() + Duration::seconds(10),
        )
        .await
        .unwrap();
    let clock = sqlx::AnyPool::connect(url).await.unwrap();
    sqlx::query("UPDATE jobs SET ready_at=0 WHERE tenant_id=$1 AND id=$2")
        .bind(a.tenant_id.to_string())
        .bind(fired.job_id.unwrap().to_string())
        .execute(&clock)
        .await
        .unwrap();
    clock.close().await;
    store.recover_jobs(&a).await.unwrap();
    let retried = store
        .claim_job(&a, fired.job_id.unwrap(), 120)
        .await
        .unwrap();
    assert_eq!(retried.job.attempt, 2);
    assert_eq!(
        store
            .fire_intention(&a, fired.id, None, false, None)
            .await
            .unwrap()
            .job_id,
        fired.job_id
    );
    finish_assigned(store, service, &a, &retried, WorkStatus::Complete).await;
    let pending = store
        .complete_intention(&a, o.id, true, None)
        .await
        .unwrap();
    assert_eq!(pending.state, IntentionState::Fired);
    assert!(
        pending
            .unresolved
            .iter()
            .any(|s| s.contains("confirmation"))
    );
    let wrong = Authority {
        actor_id: Uuid::now_v7(),
        ..a.clone()
    };
    assert!(matches!(
        store.confirm_intention(&wrong, o.id).await,
        Err(Error::Forbidden)
    ));
    store.confirm_intention(&a, o.id).await.unwrap();
    assert_eq!(
        store
            .complete_intention(&a, o.id, true, None)
            .await
            .unwrap()
            .state,
        IntentionState::Completed
    );
    assert_eq!(
        store
            .cancel_intention(&a, o.id, "Too late".into())
            .await
            .unwrap()
            .state,
        IntentionState::Completed
    );
    // Host time, not the text of an answer, decides late delivery.
    let mut clock = fired.clone();
    let now = Utc::now();
    clock.expires_at = Some(now);
    assert!(!clock.accepts_completion(now, now - Duration::seconds(1)));
    if let MemoryContent::Intention { plan: Some(p), .. } = &mut clock.definition.record.content {
        p.completion.delivery_grace_seconds = Some(30);
    }
    assert!(clock.accepts_completion(now + Duration::seconds(2), now - Duration::seconds(1)));
    assert!(!clock.accepts_completion(now + Duration::seconds(2), now));
    for state in [
        IntentionState::Pending,
        IntentionState::Armed,
        IntentionState::Completed,
        IntentionState::Expired,
        IntentionState::Cancelled,
    ] {
        clock.state = state;
        assert!(!clock.accepts_completion(now, now - Duration::seconds(1)));
        assert_eq!(clock.terminal_due(), clock.expires_at);
    }
    // A successful job with a partial application result cannot fulfil the obligation.
    let partial = add_intention(store, &a, r.clone()).await;
    store
        .arm_intention(&a, partial.id, true, None)
        .await
        .unwrap();
    let partial = store
        .fire_intention(&a, partial.id, None, false, None)
        .await
        .unwrap();
    finish(store, service, &a, &partial, WorkStatus::Partial).await;
    assert_eq!(
        store
            .complete_intention(&a, partial.id, true, None)
            .await
            .unwrap()
            .state,
        IntentionState::Fired
    );
    store
        .cancel_intention(&a, partial.id, "No longer needed".into())
        .await
        .unwrap();
    let late = store
        .complete_intention(&a, partial.id, true, None)
        .await
        .unwrap();
    assert_eq!(late.state, IntentionState::Cancelled);
    assert_eq!(late.evidence.len(), 1);
    let uncertain = add_intention(store, &a, r.clone()).await;
    store
        .arm_intention(&a, uncertain.id, true, None)
        .await
        .unwrap();
    let uncertain = store
        .fire_intention(&a, uncertain.id, None, false, None)
        .await
        .unwrap();
    let assigned = store
        .claim_job(&a, uncertain.job_id.unwrap(), 120)
        .await
        .unwrap();
    let effect = store
        .prepare_effect(
            &a,
            &permit(&assigned),
            EffectRequest {
                logical_operation_id: format!("{}/test", assigned.job.operation_id),
                kind: "test effect".into(),
                invocation_id: "test".into(),
                replay: ReplayClass::Reconcile,
                arguments: serde_json::json!({}),
            },
        )
        .await
        .unwrap();
    store
        .begin_effect(&a, &permit(&assigned), effect.id)
        .await
        .unwrap();
    store
        .report_effect(
            &a,
            &permit(&assigned),
            effect.id,
            EffectState::OutcomeUnknown,
            serde_json::json!({"transport":"acknowledgement lost"}),
        )
        .await
        .unwrap();
    finish_assigned(store, service, &a, &assigned, WorkStatus::Partial).await;
    let checked = store
        .complete_intention(&a, uncertain.id, true, None)
        .await
        .unwrap();
    assert_eq!(checked.state, IntentionState::Fired);
    assert!(
        checked
            .unresolved
            .iter()
            .any(|s| s == "Execution effects remain unresolved")
    );
    store
        .cancel_intention(&a, uncertain.id, "Stop uncertain work".into())
        .await
        .unwrap();
    assert!(
        store
            .intention(&a, uncertain.id)
            .await
            .unwrap()
            .unresolved
            .iter()
            .any(|s| s.contains("reconciliation"))
    );
    let mut required = r.clone();
    if let MemoryContent::Intention { plan: Some(p), .. } = &mut required.content {
        p.completion.required_effects = vec!["verified-action".into()];
    }
    let required = add_intention(store, &a, required).await;
    store
        .arm_intention(&a, required.id, true, None)
        .await
        .unwrap();
    let required = store
        .fire_intention(&a, required.id, None, false, None)
        .await
        .unwrap();
    let assigned = store
        .claim_job(&a, required.job_id.unwrap(), 120)
        .await
        .unwrap();
    let effect = store
        .prepare_effect(
            &a,
            &permit(&assigned),
            EffectRequest {
                logical_operation_id: format!("{}/verified-action", assigned.job.operation_id),
                invocation_id: "verified-action".into(),
                kind: "write".into(),
                replay: ReplayClass::Reconcile,
                arguments: serde_json::json!({}),
            },
        )
        .await
        .unwrap();
    store
        .begin_effect(&a, &permit(&assigned), effect.id)
        .await
        .unwrap();
    store
        .report_effect(
            &a,
            &permit(&assigned),
            effect.id,
            EffectState::Succeeded,
            serde_json::json!({"verified":true}),
        )
        .await
        .unwrap();
    finish_assigned(store, service, &a, &assigned, WorkStatus::Complete).await;
    store.confirm_intention(&a, required.id).await.unwrap();
    assert_eq!(
        store
            .complete_intention(&a, required.id, true, None)
            .await
            .unwrap()
            .state,
        IntentionState::Completed
    );
    let racing = add_intention(store, &a, r.clone()).await;
    store
        .arm_intention(&a, racing.id, true, None)
        .await
        .unwrap();
    let racing = store
        .fire_intention(&a, racing.id, None, false, None)
        .await
        .unwrap();
    finish(store, service, &a, &racing, WorkStatus::Complete).await;
    store.confirm_intention(&a, racing.id).await.unwrap();
    let (complete, cancel) = tokio::join!(
        store.complete_intention(&a, racing.id, true, None),
        store.cancel_intention(&a, racing.id, "Cancel at completion".into())
    );
    let complete = complete.unwrap();
    let cancel = cancel.unwrap();
    assert_eq!(complete.state, cancel.state);
    assert!(matches!(
        complete.state,
        IntentionState::Completed | IntentionState::Cancelled
    ));
    // Expiry is controlled in this disposable fixture without wall-clock sleeps.
    let expired = add_intention(store, &a, r.clone()).await;
    let mut data = expired.clone();
    data.expires_at = Some(Utc::now() - Duration::seconds(1));
    let pool = sqlx::AnyPool::connect(url).await.unwrap();
    sqlx::query("UPDATE intention_occurrences SET data=$1 WHERE tenant_id=$2 AND id=$3")
        .bind(serde_json::to_string(&data).unwrap())
        .bind(a.tenant_id.to_string())
        .bind(data.id.to_string())
        .execute(&pool)
        .await
        .unwrap();
    store.sweep_intentions(&a, 100).await.unwrap();
    assert_eq!(
        store.intention(&a, expired.id).await.unwrap().state,
        IntentionState::Expired
    );
    let inaccessible = Authority {
        tenant_id: Uuid::now_v7(),
        ..a.clone()
    };
    assert!(store.intention(&inaccessible, o.id).await.is_err());
    // Invalid job creation rolls back both occurrence state and outbox writes.
    let mut bad = r.clone();
    if let MemoryContent::Intention { plan: Some(p), .. } = &mut bad.content {
        p.execution.limits.root_budget_id = Uuid::now_v7();
    }
    let bad = add_intention(store, &a, bad).await;
    store.arm_intention(&a, bad.id, true, None).await.unwrap();
    let cursor = store.position().await.unwrap();
    assert!(
        store
            .fire_intention(&a, bad.id, None, false, None)
            .await
            .is_err()
    );
    assert_eq!(store.position().await.unwrap(), cursor);
    assert_eq!(
        store.intention(&a, bad.id).await.unwrap().state,
        IntentionState::Armed
    );
    store
        .cancel_intention(&a, bad.id, "Bad test plan".into())
        .await
        .unwrap();
    // Events occurring between definition creation and arming are not lost.
    let target = store
        .create_memory(&a, record(&a, &command.payload.brief.policy))
        .await
        .unwrap();
    let mut event_definition = r.clone();
    if let MemoryContent::Intention { plan: Some(p), .. } = &mut event_definition.content {
        p.trigger = IntentionTrigger::Event {
            resource_id: target.reference.memory_id,
            event_kind: "memory_revised".into(),
        };
    }
    let event_occurrence = add_intention(store, &a, event_definition).await;
    store
        .revise_memory(&a, &target.reference, target.record)
        .await
        .unwrap();
    store.sweep_intentions(&a, 100).await.unwrap();
    let event_fired = store.intention(&a, event_occurrence.id).await.unwrap();
    assert_eq!(event_fired.state, IntentionState::Fired);
    assert!(event_fired.trigger_event.is_some());
    // Downtime skips old recurring windows, rather than creating a burst of jobs.
    let mut recurring = r.clone();
    if let MemoryContent::Intention { plan: Some(p), .. } = &mut recurring.content {
        p.trigger = IntentionTrigger::Time {
            at: Utc::now() - Duration::minutes(5),
        };
        p.recurrence_seconds = Some(60);
    }
    let first = add_intention(store, &a, recurring).await;
    store.sweep_intentions(&a, 100).await.unwrap();
    let occurrences = store
        .intentions(&a, first.definition.reference.memory_id)
        .await
        .unwrap();
    assert_eq!(occurrences.len(), 2);
    assert_eq!(occurrences[0].state, IntentionState::Expired);
    assert!(occurrences[1].cycle >= 5);
    assert_eq!(occurrences[1].state, IntentionState::Fired);
    pool.close().await;
}
fn disposition(kind: ChangeKind) -> MaintenanceDisposition {
    MaintenanceDisposition {
        kind,
        comparison: None,
        support: Default::default(),
        indirect: Default::default(),
        conflicts: Default::default(),
    }
}
async fn review(
    store: &Store,
    a: &Authority,
    command: &Command<SubmitJob>,
    before: &MemoryVersion,
    after: Option<RecordDraft>,
    removed: Vec<SourceRef>,
) -> (Fence, MaintenanceReview) {
    let mut c = command.clone();
    c.request_id = Uuid::now_v7();
    c.payload.brief.process = Process::Maintenance;
    c.payload.brief.inputs.memories = vec![before.reference.clone()];
    let job = store.submit_job(a, c).await.unwrap();
    let assignment = store.claim_job(a, job.id, 120).await.unwrap();
    let f = permit(&assignment);
    let r = store
        .maintenance_review(
            a,
            &f,
            Uuid::now_v7(),
            MaintenanceRequest {
                before: before.reference.clone(),
                after,
                removed_sources: removed,
                candidates: vec![],
                reason: "Investigate source change".into(),
            },
        )
        .await
        .unwrap();
    (f, r)
}
async fn history(store: &Store) {
    let (a, command) = setup(store).await;
    let mut c = record(&a, &command.payload.brief.policy);
    c.label = "C: pressure on instance".into();
    c.valid_time = ValidTime::Interval {
        from: Some(Utc::now() - Duration::days(2)),
        to: None,
    };
    let c = store.create_memory(&a, c).await.unwrap();
    let before_change = store.position().await.unwrap();
    let boundary = Utc::now() - Duration::days(1);
    let mut d = c.record.clone();
    d.label = "D: pressure on type".into();
    d.content = MemoryContent::Knowledge {
        statement: "Pressure moved to type".into(),
        subject: None,
        predicate: None,
        uncertainty: vec![],
        examined_coverage: vec![],
    };
    d.valid_time = ValidTime::Interval {
        from: Some(boundary),
        to: None,
    };
    let (f, r) = review(store, &a, &command, &c, Some(d), vec![]).await;
    let changed = store
        .commit_maintenance(&a, &f, &r, disposition(ChangeKind::WorldChange))
        .await
        .unwrap();
    assert_eq!(changed.changed.len(), 2);
    assert_eq!(changed.notices.len(), 1);
    let again = store
        .commit_maintenance(&a, &f, &r, disposition(ChangeKind::WorldChange))
        .await
        .unwrap();
    assert_eq!(again.changed[1].reference, changed.changed[1].reference);
    let past = store
        .memories(
            &a,
            &MemoryQuery {
                valid_at: Some(boundary - Duration::hours(1)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(
        past.iter()
            .any(|m| m.reference.memory_id == c.reference.memory_id)
    );
    assert!(
        !past
            .iter()
            .any(|m| m.reference.memory_id == changed.changed[1].reference.memory_id)
    );
    let now = store
        .memories(
            &a,
            &MemoryQuery {
                valid_at: Some(Utc::now()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(
        !now.iter()
            .any(|m| m.reference.memory_id == c.reference.memory_id)
    );
    assert!(
        now.iter()
            .any(|m| m.reference == changed.changed[1].reference)
    );
    let prior = store
        .memories(
            &a,
            &MemoryQuery {
                recorded_as_of: Some(before_change),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(prior.len(), 1);
    assert_eq!(prior[0].reference, c.reference);
    store
        .complete_job(&a, Uuid::now_v7(), &f, result(&a))
        .await
        .unwrap();
    let successor = changed.changed[1].clone();
    let mut corrected = successor.record.clone();
    corrected.label = "Corrected type location".into();
    let (f, r) = review(store, &a, &command, &successor, Some(corrected), vec![]).await;
    let correction = store
        .commit_maintenance(&a, &f, &r, disposition(ChangeKind::Correction))
        .await
        .unwrap();
    assert_eq!(
        correction.changed[0].reference.memory_id,
        successor.reference.memory_id
    );
    assert_eq!(
        correction.changed[0].record.valid_time,
        successor.record.valid_time
    );
    let notices = store.memory_changes(&a, before_change, 100).await.unwrap();
    assert_eq!(notices.changes.len(), 2);
    // A revision after review must reject the stale maintenance decision.
    store
        .complete_job(&a, Uuid::now_v7(), &f, result(&a))
        .await
        .unwrap();
    let m = correction.changed[0].clone();
    let (f, r) = review(store, &a, &command, &m, Some(m.record.clone()), vec![]).await;
    store
        .revise_memory(&a, &m.reference, m.record.clone())
        .await
        .unwrap();
    assert!(matches!(
        store
            .commit_maintenance(&a, &f, &r, disposition(ChangeKind::Wording))
            .await,
        Err(Error::Conflict)
    ));
}
#[tokio::test]
async fn sqlite_maintenance_and_intentions() {
    let dir = tempfile::tempdir().unwrap();
    let url = format!(
        "sqlite://{}?mode=rwc",
        dir.path().join("test.sqlite").display()
    );
    let store = Store::connect(&url).await.unwrap();
    let service = ArtifactService::local(store.clone(), dir.path().join("artifacts"))
        .await
        .unwrap();
    lifecycle(&store, &url, &service).await;
    history(&store).await;
    support_and_conflicts(&store, &service).await;
    named_triggers(&store, &service).await;
    support_revoked_during_review(&store, &service).await;
    store.close().await;
}
#[tokio::test]
#[ignore = "requires disposable PostgreSQL"]
async fn postgres_maintenance_and_intentions() {
    let dir = tempfile::tempdir().unwrap();
    let url = std::env::var("MEMORY_TEST_POSTGRES_URL").unwrap();
    let store = Store::connect(&url).await.unwrap();
    let service = ArtifactService::local(store.clone(), dir.path().join("artifacts"))
        .await
        .unwrap();
    lifecycle(&store, &url, &service).await;
    history(&store).await;
    support_and_conflicts(&store, &service).await;
    named_triggers(&store, &service).await;
    support_revoked_during_review(&store, &service).await;
    store.close().await;
}
async fn sourced(
    store: &Store,
    service: &ArtifactService,
    a: &Authority,
    policy: &ConfigRef,
    label: &str,
) -> (MemoryVersion, SourceVersion) {
    let art = service
        .allocate_named(
            a,
            Uuid::now_v7(),
            ArtifactSpec {
                label: label.into(),
                scope: a.scope.clone(),
                media_type: "text/plain".into(),
                expected_bytes: 1,
                origin: Origin::Observed,
                retention_class: "test".into(),
                dependencies: vec![],
            },
        )
        .await
        .unwrap();
    service
        .upload(a, art.id, art.revision, b"x".as_slice())
        .await
        .unwrap();
    let source = SourceVersion {
        reference: SourceRef {
            source_id: Uuid::now_v7(),
            revision: "1".into(),
        },
        label: label.into(),
        scope: a.scope.clone(),
        kind: SourceKind::Document,
        owner: "fixture".into(),
        acquired_at: Utc::now(),
        acquisition_method: "fixture".into(),
        precedence: None,
        snapshot_artifact: Some(art.id),
    };
    store
        .register_source_version(a, source.clone())
        .await
        .unwrap();
    let loc = SourceLocator {
        id: Uuid::now_v7(),
        source: source.reference.clone(),
        locator: Locator::Document {
            start_line: 1,
            end_line: 1,
            page: None,
        },
    };
    store.create_source_locator(a, loc.clone()).await.unwrap();
    let mut r = record(a, policy);
    r.label = label.into();
    r.source_locators = vec![loc.id];
    (store.create_memory(a, r).await.unwrap(), source)
}
async fn edge(
    store: &Store,
    a: &Authority,
    from: &MemoryVersion,
    to: &MemoryVersion,
    kind: RelationKind,
) -> RelationVersion {
    store
        .create_relation(
            a,
            RelationDraft {
                from: from.reference.clone(),
                to: to.reference.clone(),
                kind,
                scope: a.scope.clone(),
                basis: "Fixture evidence".into(),
                evidential_status: EvidentialStatus::Observation,
                acceptance: Acceptance::Accepted,
                valid_time: ValidTime::Unknown,
            },
        )
        .await
        .unwrap()
}
async fn support_and_conflicts(store: &Store, service: &ArtifactService) {
    let (a, command) = setup(store).await;
    let policy = &command.payload.brief.policy;
    let (source, lost) = sourced(store, service, &a, policy, "Lost source").await;
    let (independent, _) = sourced(store, service, &a, policy, "Independent support").await;
    let mut derivative = record(&a, policy);
    derivative.label = "Governed derivative".into();
    derivative.derived_from = vec![source.reference.clone()];
    let derivative = store.create_memory(&a, derivative).await.unwrap();
    let mut claim = record(&a, policy);
    claim.label = "Independently supported claim".into();
    let claim = store.create_memory(&a, claim).await.unwrap();
    edge(store, &a, &source, &claim, RelationKind::Supports).await;
    edge(store, &a, &independent, &claim, RelationKind::Supports).await;
    edge(store, &a, &independent, &derivative, RelationKind::Supports).await;
    let art = service
        .inspect(&a, lost.snapshot_artifact.unwrap())
        .await
        .unwrap();
    service.revoke(&a, art.id, art.revision).await.unwrap();
    let (f, r) = review(store, &a, &command, &source, None, vec![lost.reference]).await;
    assert_eq!(r.affected.len(), 3);
    let mut d = disposition(ChangeKind::SupportRemoval);
    for s in &r.affected {
        d.support
            .insert(s.claim.reference.memory_id, "adequate".into());
    }
    let result = store.commit_maintenance(&a, &f, &r, d).await.unwrap();
    assert_eq!(
        store
            .current_memory(&a, derivative.reference.memory_id)
            .await
            .unwrap()
            .record
            .availability,
        Availability::Retired
    );
    assert_eq!(
        store
            .current_memory(&a, claim.reference.memory_id)
            .await
            .unwrap()
            .record
            .availability,
        Availability::Routine
    );
    assert_eq!(result.revalidation.len(), 2);
    store
        .complete_job(&a, Uuid::now_v7(), &f, self::result(&a))
        .await
        .unwrap();
    // A changed supporting relation invalidates a review even if the claim text is unchanged.
    let target = store
        .current_memory(&a, claim.reference.memory_id)
        .await
        .unwrap();
    let conflict = edge(
        store,
        &a,
        &target,
        &independent,
        RelationKind::ConflictsWith,
    )
    .await;
    let (f, r) = review(
        store,
        &a,
        &command,
        &target,
        Some(target.record.clone()),
        vec![],
    )
    .await;
    assert!(r.conflicts.iter().all(|c| c.claims.len() == 2));
    let mut withdrawn = conflict.relation.clone();
    withdrawn.acceptance = Acceptance::Withdrawn;
    store
        .revise_relation(&a, conflict.id, conflict.revision, withdrawn)
        .await
        .unwrap();
    assert!(matches!(
        store
            .commit_maintenance(&a, &f, &r, disposition(ChangeKind::Wording))
            .await,
        Err(Error::Conflict)
    ));
    store
        .complete_job(&a, Uuid::now_v7(), &f, self::result(&a))
        .await
        .unwrap();
    // An unresolved incompatible proposal records both positions and an open conflict.
    let (f, r) = review(
        store,
        &a,
        &command,
        &independent,
        Some(independent.record.clone()),
        vec![],
    )
    .await;
    let mut d = disposition(ChangeKind::Unresolved);
    d.comparison = Some("incompatible".into());
    let disputed = store.commit_maintenance(&a, &f, &r, d).await.unwrap();
    assert_eq!(disputed.changed.len(), 1);
    let conflicts = store
        .relations(&a, independent.reference.memory_id, &MemoryQuery::default())
        .await
        .unwrap();
    assert!(
        conflicts
            .iter()
            .any(|r| r.relation.kind == RelationKind::ConflictsWith)
    );
}
async fn named_triggers(store: &Store, service: &ArtifactService) {
    let (a, command) = setup(store).await;
    let (_, source) = sourced(
        store,
        service,
        &a,
        &command.payload.brief.policy,
        "Watched source",
    )
    .await;
    let mut r = definition(&a, &command.payload.brief);
    if let MemoryContent::Intention { plan: Some(p), .. } = &mut r.content {
        p.trigger = IntentionTrigger::SourceRevision {
            source_id: source.reference.source_id,
            after_revision: "1".into(),
        };
    }
    let source_occurrence = add_intention(store, &a, r).await;
    let mut revised = source;
    revised.reference.revision = "2".into();
    store.register_source_version(&a, revised).await.unwrap();
    store.sweep_intentions(&a, 100).await.unwrap();
    assert_eq!(
        store
            .intention(&a, source_occurrence.id)
            .await
            .unwrap()
            .state,
        IntentionState::Fired
    );
    let watched = store.submit_job(&a, command.clone()).await.unwrap();
    let mut r = definition(&a, &command.payload.brief);
    if let MemoryContent::Intention { plan: Some(p), .. } = &mut r.content {
        p.trigger = IntentionTrigger::Result { job_id: watched.id };
    }
    let result_occurrence = add_intention(store, &a, r).await;
    let assigned = store.claim_job(&a, watched.id, 120).await.unwrap();
    finish_assigned(store, service, &a, &assigned, WorkStatus::Complete).await;
    store.sweep_intentions(&a, 100).await.unwrap();
    assert_eq!(
        store
            .intention(&a, result_occurrence.id)
            .await
            .unwrap()
            .state,
        IntentionState::Fired
    );
    let pending = add_intention(store, &a, definition(&a, &command.payload.brief)).await;
    let mut c = command.clone();
    c.request_id = Uuid::now_v7();
    c.payload.brief.process = Process::Maintenance;
    c.payload.brief.inputs.memories = vec![pending.definition.reference.clone()];
    let review_job = store.submit_job(&a, c).await.unwrap();
    let assigned = store.claim_job(&a, review_job.id, 120).await.unwrap();
    let mut stale = permit(&assigned);
    stale.epoch += 1;
    assert!(matches!(
        store
            .arm_intention(&a, pending.id, true, Some(&stale))
            .await,
        Err(Error::Conflict)
    ));
    let mut escalation = pending.definition.record.clone();
    if let MemoryContent::Intention { plan: Some(p), .. } = &mut escalation.content {
        p.execution
            .capabilities
            .tools
            .push("unauthorised_tool".into());
    }
    assert!(matches!(
        store
            .maintenance_review(
                &a,
                &permit(&assigned),
                Uuid::now_v7(),
                MaintenanceRequest {
                    before: pending.definition.reference,
                    after: Some(escalation),
                    removed_sources: vec![],
                    candidates: vec![],
                    reason: "Escalation attempt".into()
                }
            )
            .await,
        Err(Error::Forbidden)
    ));
}
async fn support_revoked_during_review(store: &Store, service: &ArtifactService) {
    let (a, command) = setup(store).await;
    let policy = &command.payload.brief.policy;
    let (before, _) = sourced(store, service, &a, policy, "Changing source").await;
    let (remaining, source) = sourced(store, service, &a, policy, "Remaining source").await;
    let claim = store.create_memory(&a, record(&a, policy)).await.unwrap();
    edge(store, &a, &before, &claim, RelationKind::Supports).await;
    edge(store, &a, &remaining, &claim, RelationKind::Supports).await;
    let (f, r) = review(
        store,
        &a,
        &command,
        &before,
        Some(before.record.clone()),
        vec![],
    )
    .await;
    let artifact = service
        .inspect(&a, source.snapshot_artifact.unwrap())
        .await
        .unwrap();
    service
        .revoke(&a, artifact.id, artifact.revision)
        .await
        .unwrap();
    let mut d = disposition(ChangeKind::Correction);
    d.support
        .insert(claim.reference.memory_id, "adequate".into());
    assert!(
        store.commit_maintenance(&a, &f, &r, d).await.is_err(),
        "Revoked remaining evidence must invalidate the review"
    );
    assert_eq!(
        store
            .current_memory(&a, before.reference.memory_id)
            .await
            .unwrap()
            .reference,
        before.reference
    );
}
