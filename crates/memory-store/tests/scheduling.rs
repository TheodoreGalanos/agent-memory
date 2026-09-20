// ABOUTME: Store-level TDD coverage for the scheduling sweeps: consolidation cohorts, procedure
// ABOUTME: qualification, correction/source-revision maintenance and the intention settle-check.
use chrono::{Duration, Utc};
use memory_domain::{consolidation::*, contracts::*, coordination::*, intentions::*, records::*};
use memory_store::Store;
use uuid::Uuid;

#[path = "scheduling/backlog.rs"]
mod backlog;

struct Fixture {
    auth: Authority,
    policy: ConfigRef,
    project_scope: Scope,
    budget_id: Uuid,
}

async fn setup(store: &Store) -> Fixture {
    let project_id = Uuid::now_v7();
    let task_id = Uuid::now_v7();
    // An unrestricted (project-wide) authority, exactly as an Administrator credential's scope
    // would be: it can write any narrower, task-scoped resource within the project.
    let auth = Authority {
        tenant_id: Uuid::now_v7(),
        actor_id: Uuid::now_v7(),
        scope: Scope {
            task_id: None,
            project_id: Some(project_id),
            ..Default::default()
        },
    };
    let project_scope = auth.scope.clone();
    // A project-wide budget approved ahead of time; individual task jobs stay task-scoped but
    // reference this same budget, which is how a broader (project-scoped) scheduled job is funded.
    let budget = store
        .create_budget(
            &auth,
            Budget {
                id: Uuid::now_v7(),
                scope: project_scope.clone(),
                limit: Resources {
                    tokens: 1_000_000,
                    provider_calls: 100,
                    output_bytes: 1_000_000,
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
    let policy = store
        .create_policy(
            &auth,
            "Scheduling test policy",
            project_scope.clone(),
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
                consolidation: Some(ConsolidationPolicy {
                    min_independent_sources: 1,
                    min_held_out_groups: 1,
                    minimum_success_rate: 1.0,
                    maximum_regression: 0.0,
                    maximum_cost_microunits: 1000,
                    evaluator_profile: profile(),
                    min_cohort_size: 2,
                    max_cohort_size: 3,
                }),
            },
            ValidTime::Unknown,
        )
        .await
        .unwrap();
    // A settled root task job under the shared budget: the "recent contributing task" whose
    // budget funds cohort/maintenance work in the broader project scope.
    submit_job(
        store,
        &auth,
        base_brief(&auth, &policy.reference, auth.scope.clone(), task_id),
        budget.id,
        Utc::now() + Duration::minutes(30),
    )
    .await;
    Fixture {
        auth,
        policy: policy.reference,
        project_scope,
        budget_id: budget.id,
    }
}

fn profile() -> ConfigRef {
    ConfigRef {
        id: Uuid::now_v7(),
        revision: std::num::NonZeroU32::new(1).unwrap(),
        label: "test-profile".into(),
    }
}

fn base_brief(_auth: &Authority, policy: &ConfigRef, mut scope: Scope, task_id: Uuid) -> WorkBrief {
    scope.task_id = Some(task_id);
    WorkBrief {
        schema_version: WireVersion::V1,
        purpose: "Scheduling test work".into(),
        process: Process::Investigation,
        task_id,
        frame_ref: None,
        definitions: vec![],
        scope,
        inputs: WorkInputs {
            sources: vec![],
            memories: vec![],
            artifacts: vec![],
        },
        capabilities: Capabilities {
            sources: vec![],
            queries: vec![],
            tools: vec![],
        },
        known_conflicts: vec![],
        evidence_cutoff: Utc::now(),
        freshness_requirement: "none".into(),
        output_criteria: vec!["Scheduling test output".into()],
        limits: WorkLimits {
            root_budget_id: Uuid::nil(),
            max_provider_attempts: std::num::NonZeroU32::new(10).unwrap(),
            max_tokens: std::num::NonZeroU32::new(10_000).unwrap(),
            max_output_bytes: std::num::NonZeroU32::new(10_000).unwrap(),
            max_child_depth: 1,
            max_child_concurrency: 1,
        },
        policy: policy.clone(),
        profile: profile(),
        retention_policy: "test".into(),
        disclosure_policy: "test".into(),
        scheduled: None,
    }
}

async fn submit_job(
    store: &Store,
    auth: &Authority,
    mut brief: WorkBrief,
    budget_id: Uuid,
    deadline: chrono::DateTime<Utc>,
) -> Uuid {
    brief.limits.root_budget_id = budget_id;
    let command = Command {
        request_id: Uuid::now_v7(),
        command_version: WireVersion::V1,
        tenant_id: auth.tenant_id,
        actor_id: auth.actor_id,
        scope: brief.scope.clone(),
        job_id: None,
        session_id: None,
        operation_id: None,
        lane_id: None,
        invocation_id: None,
        expected_revisions: vec![],
        deadline,
        budget_id,
        lease_epoch: None,
        payload: SubmitJob {
            brief,
            parent_id: None,
            max_attempts: 3,
            retain_until: deadline + Duration::days(1),
        },
    };
    store.submit_job(auth, command).await.unwrap().id
}

fn candidate(scope: &Scope, policy: &ConfigRef, label: &str) -> RecordDraft {
    RecordDraft {
        label: label.into(),
        scope: scope.clone(),
        content: MemoryContent::Knowledge {
            statement: format!("{label} statement"),
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
            reason: "Test candidate".into(),
            constraints: vec![],
            required_evidence: vec![],
            expires_at: None,
        },
    }
}

async fn consolidation_cohort(store: &Store) {
    let f = setup(store).await;
    let r1 = store
        .create_memory(&f.auth, candidate(&f.project_scope, &f.policy, "first"))
        .await
        .unwrap();
    // Below the configured minimum (2): nothing is scheduled.
    assert_eq!(store.schedule_consolidation(&f.auth, 10).await.unwrap(), 0);
    assert!(
        store
            .queued_jobs(&f.auth, &[Process::Consolidation], 10)
            .await
            .unwrap()
            .is_empty()
    );
    let r2 = store
        .create_memory(&f.auth, candidate(&f.project_scope, &f.policy, "second"))
        .await
        .unwrap();
    assert_eq!(store.schedule_consolidation(&f.auth, 10).await.unwrap(), 1);
    let queued = store
        .queued_jobs(&f.auth, &[Process::Consolidation], 10)
        .await
        .unwrap();
    assert_eq!(queued.len(), 1);
    let cohort_job = store.job(&f.auth, queued[0]).await.unwrap();
    assert_eq!(cohort_job.spec.brief.process, Process::Consolidation);
    assert_eq!(cohort_job.spec.brief.policy, f.policy);
    assert_eq!(cohort_job.spec.brief.limits.root_budget_id, f.budget_id);
    let members: std::collections::BTreeSet<_> = cohort_job
        .spec
        .brief
        .inputs
        .memories
        .iter()
        .map(|m| m.memory_id)
        .collect();
    assert!(members.contains(&r1.reference.memory_id));
    assert!(members.contains(&r2.reference.memory_id));
    // A fresh, distinct task id was invented for the cohort (the policy scope carries none).
    assert_eq!(
        cohort_job.spec.brief.scope.task_id,
        Some(cohort_job.spec.brief.task_id)
    );
    assert_ne!(cohort_job.spec.brief.task_id, Uuid::nil());
    // Idempotent: the same two records do not form a second cohort.
    assert_eq!(store.schedule_consolidation(&f.auth, 10).await.unwrap(), 0);
    assert_eq!(
        store
            .queued_jobs(&f.auth, &[Process::Consolidation], 10)
            .await
            .unwrap()
            .len(),
        1
    );
    // A later sweep still forms a new cohort once enough new records accumulate.
    store
        .create_memory(&f.auth, candidate(&f.project_scope, &f.policy, "third"))
        .await
        .unwrap();
    assert_eq!(store.schedule_consolidation(&f.auth, 10).await.unwrap(), 0);
    store
        .create_memory(&f.auth, candidate(&f.project_scope, &f.policy, "fourth"))
        .await
        .unwrap();
    assert_eq!(store.schedule_consolidation(&f.auth, 10).await.unwrap(), 1);
    let queued = store
        .queued_jobs(&f.auth, &[Process::Consolidation], 10)
        .await
        .unwrap();
    assert_eq!(queued.len(), 2);
    let second_id = *queued.iter().find(|id| **id != cohort_job.id).unwrap();
    let second_cohort = store.job(&f.auth, second_id).await.unwrap();
    assert_ne!(
        second_cohort.spec.brief.task_id,
        cohort_job.spec.brief.task_id
    );
}

async fn concurrent_consolidation_sweeps(store: &Store) {
    let f = setup(store).await;
    for label in ["first", "second"] {
        store
            .create_memory(&f.auth, candidate(&f.project_scope, &f.policy, label))
            .await
            .unwrap();
    }
    let (first, second) = tokio::join!(
        store.schedule_consolidation(&f.auth, 10),
        store.schedule_consolidation(&f.auth, 10),
    );
    assert_eq!(first.unwrap() + second.unwrap(), 1);
    assert_eq!(
        store
            .queued_jobs(&f.auth, &[Process::Consolidation], 10)
            .await
            .unwrap()
            .len(),
        1
    );
}

async fn qualification_after_consolidation(store: &Store, raw: &sqlx::AnyPool) {
    let f = setup(store).await;
    let task_id = Uuid::now_v7();
    let mut scoped = f.project_scope.clone();
    scoped.task_id = Some(task_id);
    let mut draft = candidate(&scoped, &f.policy, "procedure");
    draft.content = procedure_content();
    let candidate_version = store.create_memory(&f.auth, draft).await.unwrap();
    let consolidation_job = submit_job(
        store,
        &f.auth,
        base_brief(&f.auth, &f.policy, scoped.clone(), task_id),
        f.budget_id,
        Utc::now() + Duration::minutes(30),
    )
    .await;
    // Earlier reviews that retained nothing must not hide later candidates at the page boundary.
    for _ in 0..500 {
        let review_id = Uuid::now_v7();
        insert_consolidation_review(
            raw,
            &f.auth,
            review_id,
            consolidation_job,
            &ConsolidationResult {
                review_id,
                record: None,
                deferred: vec![],
                independent_sources: 0,
                coverage: Coverage {
                    examined: vec![],
                    unexamined: vec![],
                },
            },
        )
        .await;
    }
    let review_id = Uuid::now_v7();
    let result = ConsolidationResult {
        review_id,
        record: Some(candidate_version.clone()),
        deferred: vec![],
        independent_sources: 1,
        coverage: Coverage {
            examined: vec!["fixture".into()],
            unexamined: vec![],
        },
    };
    insert_consolidation_review(raw, &f.auth, review_id, consolidation_job, &result).await;
    assert_eq!(store.schedule_qualification(&f.auth, 10).await.unwrap(), 1);
    let mark = store
        .scheduled_work(
            &f.auth,
            "qualification",
            candidate_version.reference.memory_id,
        )
        .await
        .unwrap()
        .expect("qualification trigger recorded");
    let job_id = mark.job_id.expect("qualification job scheduled");
    let job = store.job(&f.auth, job_id).await.unwrap();
    assert_eq!(job.spec.brief.process, Process::Evaluation);
    assert_eq!(
        job.spec.brief.inputs.memories,
        vec![candidate_version.reference.clone()]
    );
    assert_eq!(job.spec.brief.limits.root_budget_id, f.budget_id);
    // Idempotent: the same candidate does not get a second qualification job.
    assert_eq!(store.schedule_qualification(&f.auth, 10).await.unwrap(), 0);
    assert_eq!(
        store
            .scheduled_work(
                &f.auth,
                "qualification",
                candidate_version.reference.memory_id
            )
            .await
            .unwrap()
            .unwrap()
            .job_id,
        Some(job_id)
    );
}

async fn qualification_unschedulable_on_expired_budget(store: &Store, raw: &sqlx::AnyPool) {
    let f = setup(store).await;
    let task_id = Uuid::now_v7();
    let mut scoped = f.project_scope.clone();
    scoped.task_id = Some(task_id);
    let mut draft = candidate(&scoped, &f.policy, "procedure-expiring");
    draft.content = procedure_content();
    let candidate_version = store.create_memory(&f.auth, draft).await.unwrap();
    // A budget whose deadline leaves no working time for a newly scheduled job.
    let tight_budget = store
        .create_budget(
            &f.auth,
            Budget {
                id: Uuid::now_v7(),
                scope: scoped.clone(),
                limit: Resources {
                    tokens: 1000,
                    provider_calls: 4,
                    output_bytes: 1024,
                    ..Default::default()
                },
                final_result_reserve: Resources {
                    tokens: 10,
                    output_bytes: 128,
                    ..Default::default()
                },
                deadline: Utc::now() + Duration::seconds(5),
                max_child_depth: 1,
                max_child_concurrency: 1,
                pricing_revision: "test/no-charge".into(),
            },
        )
        .await
        .unwrap();
    let consolidation_job = submit_job(
        store,
        &f.auth,
        base_brief(&f.auth, &f.policy, scoped.clone(), task_id),
        tight_budget.id,
        tight_budget.deadline,
    )
    .await;
    let review_id = Uuid::now_v7();
    let result = ConsolidationResult {
        review_id,
        record: Some(candidate_version.clone()),
        deferred: vec![],
        independent_sources: 1,
        coverage: Coverage {
            examined: vec![],
            unexamined: vec![],
        },
    };
    insert_consolidation_review(raw, &f.auth, review_id, consolidation_job, &result).await;
    assert_eq!(store.schedule_qualification(&f.auth, 10).await.unwrap(), 0);
    let mark = store
        .scheduled_work(
            &f.auth,
            "qualification",
            candidate_version.reference.memory_id,
        )
        .await
        .unwrap()
        .expect("trigger recorded even when unschedulable");
    assert!(mark.job_id.is_none());
    assert!(mark.unschedulable.is_some());
}

fn procedure_content() -> MemoryContent {
    MemoryContent::Procedure {
        purpose: "Look up a rated pressure".into(),
        capabilities: vec![],
        applicability: vec![],
        exclusions: vec![],
        method: ProcedureForm::Advisory {
            steps: vec!["Read the field".into()],
            evidence_criteria: vec!["Field value observed directly".into()],
        },
        counterexamples: vec![],
        contract: None,
    }
}

async fn maintenance_on_correction_and_causal_skip(store: &Store, raw: &sqlx::AnyPool) {
    let f = setup(store).await;
    let base = store
        .create_memory(&f.auth, candidate(&f.project_scope, &f.policy, "corrected"))
        .await
        .unwrap();
    let dependent = store
        .create_memory(&f.auth, candidate(&f.project_scope, &f.policy, "dependent"))
        .await
        .unwrap();
    store
        .create_relation(
            &f.auth,
            RelationDraft {
                from: dependent.reference.clone(),
                to: base.reference.clone(),
                kind: RelationKind::DependsOn,
                scope: f.project_scope.clone(),
                basis: "Cites the base record".into(),
                evidential_status: EvidentialStatus::Inference,
                acceptance: Acceptance::Accepted,
                valid_time: ValidTime::Unknown,
            },
        )
        .await
        .unwrap();
    // Non-correction history must not monopolize each sweep's first page.
    for index in 0..500 {
        let notice = memory_domain::maintenance::MemoryChangeNotice {
            id: Uuid::now_v7(),
            cursor: index + 1,
            previous: base.reference.clone(),
            current: base.reference.clone(),
            kind: memory_domain::maintenance::ChangeKind::Wording,
            reason: "Historical wording change".into(),
            scope: f.project_scope.clone(),
            valid_time: ValidTime::Unknown,
        };
        sqlx::query("INSERT INTO memory_changes(tenant_id,id,cursor,resource_id,data) VALUES($1,$2,$3,$4,$5)")
            .bind(f.auth.tenant_id.to_string()).bind(notice.id.to_string()).bind(i64::from(index + 1))
            .bind(base.reference.memory_id.to_string()).bind(serde_json::to_string(&notice).unwrap())
            .execute(raw).await.unwrap();
    }
    // Keep the store clock beyond the inserted historical notices.
    sqlx::query("UPDATE commit_clock SET sequence=sequence+500 WHERE id=1")
        .execute(raw)
        .await
        .unwrap();
    let mut corrected = base.record.clone();
    if let MemoryContent::Knowledge { statement, .. } = &mut corrected.content {
        *statement = "Corrected statement".into();
    }
    store
        .user_request(
            &f.auth,
            memory_domain::interaction::UserRequest::Mutate {
                request_id: Uuid::now_v7(),
                mutation: Box::new(memory_domain::interaction::UserMutation::Correct {
                    expected: base.reference.clone(),
                    record: Box::new(corrected),
                }),
            },
        )
        .await
        .unwrap();
    assert_eq!(store.schedule_maintenance(&f.auth, 10).await.unwrap(), 1);
    let queued = store
        .queued_jobs(&f.auth, &[Process::Maintenance], 10)
        .await
        .unwrap();
    assert_eq!(queued.len(), 1);
    let job = store.job(&f.auth, queued[0]).await.unwrap();
    let members: std::collections::BTreeSet<_> = job
        .spec
        .brief
        .inputs
        .memories
        .iter()
        .map(|m| m.memory_id)
        .collect();
    assert!(members.contains(&base.reference.memory_id));
    assert!(members.contains(&dependent.reference.memory_id));
    // The scheduler carries a fully-derived `MaintenanceRequest` directly on the brief (no manifest
    // artifact, since it holds no job fence to publish one): the already-corrected record is `before`,
    // its dependent is a held-out `candidate`. `review_maintenance` rejects a bare revalidation with no
    // proposed revision, so `after` repeats `before`'s own content with only the reason updated — a
    // wording-only revision that still runs a fresh J13-J16 review of the record and its dependents.
    match job.spec.brief.scheduled.as_deref() {
        Some(ScheduledInputs::MaintenanceCorrection { request }) => {
            assert_eq!(request.before.memory_id, base.reference.memory_id);
            let after = request.after.as_ref().expect("a wording-only revision");
            assert_eq!(after.label, base.record.label);
            assert_eq!(after.decision.reason, request.reason);
            assert_eq!(request.candidates.len(), 1);
            assert_eq!(
                request.candidates[0].memory_id,
                dependent.reference.memory_id
            );
            assert!(!request.reason.trim().is_empty());
        }
        other => panic!("expected a scheduled maintenance correction request, got {other:?}"),
    }
    // Idempotent: the same correction does not schedule a second job.
    assert_eq!(store.schedule_maintenance(&f.auth, 10).await.unwrap(), 0);

    // A correction whose causal origin is itself a maintenance job over the same record is
    // never rescheduled, so a module cannot reprocess its own unchanged output forever.
    let origin_memory = store
        .create_memory(
            &f.auth,
            candidate(&f.project_scope, &f.policy, "self-correcting"),
        )
        .await
        .unwrap();
    let maintenance_task_id = Uuid::now_v7();
    let mut maintenance_brief = base_brief(
        &f.auth,
        &f.policy,
        f.project_scope.clone(),
        maintenance_task_id,
    );
    maintenance_brief.process = Process::Maintenance;
    let maintenance_job = submit_job(
        store,
        &f.auth,
        maintenance_brief,
        f.budget_id,
        Utc::now() + Duration::minutes(30),
    )
    .await;
    let mut recorrected = origin_memory.record.clone();
    if let MemoryContent::Knowledge { statement, .. } = &mut recorrected.content {
        *statement = "Maintained statement".into();
    }
    store
        .user_request(
            &f.auth,
            memory_domain::interaction::UserRequest::Mutate {
                request_id: Uuid::now_v7(),
                mutation: Box::new(memory_domain::interaction::UserMutation::Correct {
                    expected: origin_memory.reference.clone(),
                    record: Box::new(recorrected),
                }),
            },
        )
        .await
        .unwrap();
    sqlx::query("UPDATE memory_changes SET causal_job_id=$1 WHERE tenant_id=$2 AND resource_id=$3")
        .bind(maintenance_job.to_string())
        .bind(f.auth.tenant_id.to_string())
        .bind(origin_memory.reference.memory_id.to_string())
        .execute(raw)
        .await
        .unwrap();
    assert_eq!(store.schedule_maintenance(&f.auth, 10).await.unwrap(), 0);
    let notice_id = latest_notice_id(raw, &f.auth, origin_memory.reference.memory_id).await;
    let mark = store
        .scheduled_work(&f.auth, "maintenance-correction", notice_id)
        .await
        .unwrap()
        .expect("causal-origin skip is recorded");
    assert!(mark.job_id.is_none());
    assert_eq!(mark.causal_origin_job_id, Some(maintenance_job));
}

async fn maintenance_on_source_revision(store: &Store) {
    let f = setup(store).await;
    let source = memory_domain::sources::SourceVersion {
        reference: SourceRef {
            source_id: Uuid::now_v7(),
            revision: "1".into(),
        },
        label: "Watched source".into(),
        scope: f.project_scope.clone(),
        kind: memory_domain::sources::SourceKind::Document,
        owner: "fixture".into(),
        acquired_at: Utc::now(),
        acquisition_method: "authored fixture".into(),
        precedence: None,
        snapshot_artifact: None,
    };
    store
        .register_source_version(&f.auth, source.clone())
        .await
        .unwrap();
    let locator = memory_domain::sources::SourceLocator {
        id: Uuid::now_v7(),
        source: source.reference.clone(),
        locator: memory_domain::sources::Locator::Document {
            start_line: 1,
            end_line: 1,
            page: None,
        },
    };
    store
        .create_source_locator(&f.auth, locator.clone())
        .await
        .unwrap();
    let mut draft = candidate(&f.project_scope, &f.policy, "sourced");
    draft.source_locators = vec![locator.id];
    let sourced = store.create_memory(&f.auth, draft).await.unwrap();
    for _ in 0..500 {
        let mut unrelated = source.clone();
        unrelated.reference.source_id = Uuid::now_v7();
        store
            .register_source_version(&f.auth, unrelated)
            .await
            .unwrap();
    }
    let mut revised_source = source.clone();
    revised_source.reference.revision = "2".into();
    store
        .register_source_version(&f.auth, revised_source)
        .await
        .unwrap();
    assert_eq!(store.schedule_maintenance(&f.auth, 10).await.unwrap(), 1);
    let queued = store
        .queued_jobs(&f.auth, &[Process::Maintenance], 10)
        .await
        .unwrap();
    assert_eq!(queued.len(), 1);
    let job = store.job(&f.auth, queued[0]).await.unwrap();
    assert!(
        job.spec
            .brief
            .inputs
            .memories
            .iter()
            .any(|m| m.memory_id == sourced.reference.memory_id)
    );
    match job.spec.brief.scheduled.as_deref() {
        Some(ScheduledInputs::MaintenanceSource { request }) => {
            assert_eq!(request.before.memory_id, sourced.reference.memory_id);
            let after = request.after.as_ref().expect("a wording-only revision");
            assert_eq!(after.decision.reason, request.reason);
            assert!(!request.reason.trim().is_empty());
        }
        other => panic!("expected a scheduled maintenance source request, got {other:?}"),
    }
    assert_eq!(store.schedule_maintenance(&f.auth, 10).await.unwrap(), 0);
}

async fn intention_settles_after_job(store: &Store) {
    let f = setup(store).await;
    let directory = tempfile::tempdir().unwrap();
    let artifacts =
        memory_store::artifacts::ArtifactService::local(store.clone(), directory.path())
            .await
            .unwrap();
    let intention_task_id = Uuid::now_v7();
    let mut execution = base_brief(
        &f.auth,
        &f.policy,
        f.project_scope.clone(),
        intention_task_id,
    );
    execution.process = Process::Maintenance;
    execution.limits.root_budget_id = f.budget_id;
    let mut draft = candidate(&f.project_scope, &f.policy, "intention");
    draft.content = MemoryContent::Intention {
        purpose: "Verify the corrected property".into(),
        owner_id: f.auth.actor_id,
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
            execution: Box::new(execution),
            ready_memories: vec![],
            ready_artifacts: vec![],
            recurrence_seconds: None,
            completion: CompletionRule {
                result_pointer: "/status".into(),
                equals: serde_json::json!("complete"),
                required_effects: vec![],
                semantic_conditions: vec![],
                confirmation_owner: None,
                delivery_grace_seconds: None,
            },
        })),
    };
    let memory = store.create_memory(&f.auth, draft).await.unwrap();
    let occurrence = store
        .intentions(&f.auth, memory.reference.memory_id)
        .await
        .unwrap()
        .remove(0);
    store
        .arm_intention(&f.auth, occurrence.id, true, None)
        .await
        .unwrap();
    let fired = store
        .sweep_intentions(&f.auth, 100)
        .await
        .unwrap()
        .occurrences
        .into_iter()
        .find(|o| o.id == occurrence.id)
        .unwrap();
    assert_eq!(fired.state, IntentionState::Fired);
    let job_id = fired.job_id.expect("intention fired a job");
    let assigned = store.claim_job(&f.auth, job_id, 120).await.unwrap();
    let fence = Fence {
        job_id: assigned.job.id,
        owner_id: assigned.owner_id,
        epoch: assigned.epoch,
    };
    store.start_job(&f.auth, &fence, None).await.unwrap();
    let bytes = b"{\"checked\":true}";
    let artifact = artifacts
        .allocate_named(
            &f.auth,
            Uuid::now_v7(),
            memory_domain::sources::ArtifactSpec {
                label: "Checked result".into(),
                scope: assigned.job.spec.brief.scope.clone(),
                media_type: "application/json".into(),
                expected_bytes: bytes.len() as u32,
                origin: Origin::AgentGenerated,
                retention_class: "test".into(),
                dependencies: vec![],
            },
        )
        .await
        .unwrap();
    artifacts
        .upload(&f.auth, artifact.id, artifact.revision, &bytes[..])
        .await
        .unwrap();
    let result = WorkResult {
        schema_version: WireVersion::V1,
        status: WorkStatus::Complete,
        examined_scope: assigned.job.spec.brief.scope.clone(),
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
        result_artifact: Some(artifact.id),
        known_effects: vec![],
        usage: Usage {
            status: UsageStatus::Known,
            input_tokens: Some(0),
            output_tokens: Some(0),
            cost: None,
        },
    };
    store
        .complete_job(&f.auth, Uuid::now_v7(), &fence, result)
        .await
        .unwrap();
    // No confirm/complete call from the test: the sweep alone must settle the occurrence.
    let swept = store
        .sweep_intentions(&f.auth, 100)
        .await
        .unwrap()
        .occurrences
        .into_iter()
        .find(|o| o.id == occurrence.id)
        .unwrap();
    assert_eq!(swept.state, IntentionState::Completed);
}

async fn insert_consolidation_review(
    raw: &sqlx::AnyPool,
    auth: &Authority,
    review_id: Uuid,
    job_id: Uuid,
    result: &ConsolidationResult,
) {
    sqlx::query(
        "INSERT INTO consolidation_reviews(tenant_id,id,job_id,data,result) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(auth.tenant_id.to_string())
    .bind(review_id.to_string())
    .bind(job_id.to_string())
    .bind("{}")
    .bind(serde_json::to_string(result).unwrap())
    .execute(raw)
    .await
    .unwrap();
}

async fn latest_notice_id(raw: &sqlx::AnyPool, auth: &Authority, resource_id: Uuid) -> Uuid {
    use sqlx::Row;
    let row = sqlx::query(
        "SELECT id FROM memory_changes WHERE tenant_id=$1 AND resource_id=$2 ORDER BY cursor DESC LIMIT 1",
    )
    .bind(auth.tenant_id.to_string())
    .bind(resource_id.to_string())
    .fetch_one(raw)
    .await
    .unwrap();
    let id_str: String = row.get("id");
    Uuid::parse_str(&id_str).unwrap()
}

async fn suite(url: &str) {
    let store = Store::connect(url).await.unwrap();
    let raw = sqlx::AnyPool::connect(url).await.unwrap();
    consolidation_cohort(&store).await;
    concurrent_consolidation_sweeps(&store).await;
    backlog::queued_processes(&store).await;
    backlog::candidate_memories(&store).await;
    qualification_after_consolidation(&store, &raw).await;
    qualification_unschedulable_on_expired_budget(&store, &raw).await;
    maintenance_on_correction_and_causal_skip(&store, &raw).await;
    maintenance_on_source_revision(&store).await;
    intention_settles_after_job(&store).await;
    raw.close().await;
    store.close().await;
}

#[tokio::test]
async fn sqlite_scheduling() {
    let temp = tempfile::tempdir().unwrap();
    suite(&format!(
        "sqlite://{}?mode=rwc",
        temp.path().join("scheduling.sqlite").display()
    ))
    .await;
}

#[tokio::test]
async fn consolidation_submission_rolls_back_if_recording_the_trigger_fails() {
    let temp = tempfile::tempdir().unwrap();
    let url = format!(
        "sqlite://{}?mode=rwc",
        temp.path().join("atomic.sqlite").display()
    );
    let store = Store::connect(&url).await.unwrap();
    let raw = sqlx::AnyPool::connect(&url).await.unwrap();
    let f = setup(&store).await;
    for label in ["first", "second"] {
        store
            .create_memory(&f.auth, candidate(&f.project_scope, &f.policy, label))
            .await
            .unwrap();
    }
    sqlx::query("CREATE TRIGGER reject_schedule BEFORE INSERT ON scheduled_work BEGIN SELECT RAISE(ABORT, 'ledger unavailable'); END")
        .execute(&raw).await.unwrap();
    assert!(store.schedule_consolidation(&f.auth, 10).await.is_err());
    assert!(
        store
            .queued_jobs(&f.auth, &[Process::Consolidation], 10)
            .await
            .unwrap()
            .is_empty(),
        "a failed scheduling transaction must not leave an unrecorded job"
    );
    sqlx::query("DROP TRIGGER reject_schedule")
        .execute(&raw)
        .await
        .unwrap();
    assert_eq!(store.schedule_consolidation(&f.auth, 10).await.unwrap(), 1);
    raw.close().await;
    store.close().await;
}

#[tokio::test]
#[ignore = "requires isolated PostgreSQL; run npm run test:store"]
async fn postgres_scheduling() {
    suite(&std::env::var("MEMORY_TEST_POSTGRES_URL").unwrap()).await;
}
