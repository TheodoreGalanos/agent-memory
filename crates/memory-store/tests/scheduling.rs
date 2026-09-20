// ABOUTME: Store-level TDD coverage for the scheduling sweeps: consolidation cohorts and the
// ABOUTME: procedure qualification job scheduled once a commit produces a candidate procedure.
use chrono::{Duration, Utc};
use memory_domain::{consolidation::*, contracts::*, coordination::*, records::*};
use memory_store::Store;
use uuid::Uuid;

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
    assert_eq!(job.spec.brief.process, Process::Consolidation);
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

async fn suite(url: &str) {
    let store = Store::connect(url).await.unwrap();
    let raw = sqlx::AnyPool::connect(url).await.unwrap();
    consolidation_cohort(&store).await;
    qualification_after_consolidation(&store, &raw).await;
    qualification_unschedulable_on_expired_budget(&store, &raw).await;
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
#[ignore = "requires isolated PostgreSQL; run npm run test:store"]
async fn postgres_scheduling() {
    suite(&std::env::var("MEMORY_TEST_POSTGRES_URL").unwrap()).await;
}
