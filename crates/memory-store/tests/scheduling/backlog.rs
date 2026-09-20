use super::*;

pub(super) async fn queued_processes(store: &Store) {
    let f = setup(store).await;
    for _ in 0..500 {
        let mut brief = base_brief(&f.auth, &f.policy, f.project_scope.clone(), Uuid::now_v7());
        brief.process = Process::Formation;
        submit_job(
            store,
            &f.auth,
            brief,
            f.budget_id,
            Utc::now() + Duration::minutes(30),
        )
        .await;
    }
    let job = submit_job(
        store,
        &f.auth,
        base_brief(&f.auth, &f.policy, f.project_scope.clone(), Uuid::now_v7()),
        f.budget_id,
        Utc::now() + Duration::minutes(30),
    )
    .await;
    assert!(
        store
            .queued_jobs(&f.auth, &[Process::Investigation], 10)
            .await
            .unwrap()
            .contains(&job),
        "jobs for disabled processes must not hide work this pool can run"
    );
}

pub(super) async fn candidate_memories(store: &Store) {
    let f = setup(store).await;
    let evidence = store
        .create_memory(&f.auth, candidate(&f.project_scope, &f.policy, "evidence"))
        .await
        .unwrap();
    for _ in 0..500 {
        let mut record = candidate(&f.project_scope, &f.policy, "already evaluated");
        record.qualification = Qualification::Evaluated {
            conditions: vec!["Test conditions".into()],
            evidence: vec![evidence.reference.clone()],
            evaluation_artifact: None,
            evaluator_profile: None,
        };
        store.create_memory(&f.auth, record).await.unwrap();
    }
    for label in ["first", "second"] {
        store
            .create_memory(&f.auth, candidate(&f.project_scope, &f.policy, label))
            .await
            .unwrap();
    }
    assert_eq!(
        store.schedule_consolidation(&f.auth, 10).await.unwrap(),
        1,
        "non-candidate history must not hide later candidates"
    );
}
