use chrono::{TimeZone, Utc};
use memory_domain::{contracts::*, records::*, sources::*};
use memory_store::{Error, Store, artifacts::ArtifactService, sources::*};
use tempfile::TempDir;
use uuid::Uuid;

fn authority() -> Authority {
    Authority {
        tenant_id: Uuid::now_v7(),
        actor_id: Uuid::now_v7(),
        scope: Scope {
            project_id: Some(Uuid::now_v7()),
            ..Scope::default()
        },
    }
}
fn policy() -> MemoryPolicy {
    MemoryPolicy {
        semantic_triggers: vec![],
        retention_purpose: "Continue project work".into(),
        allowed_uses: vec!["retrieval".into()],
        source_rules: vec![],
        evidence_requirements: vec![],
        applicability_rules: vec![],
        budget_class: "local".into(),
        scheduling_priority: 0,
        judgement_dispositions: vec![],
        notification_policy: "on_change".into(),
        qualification_requirements: vec![],
        consolidation: None,
    }
}
async fn setup(store: &Store) -> (Authority, PolicyVersion) {
    let auth = authority();
    let policy = store
        .create_policy(
            &auth,
            "Project policy",
            auth.scope.clone(),
            policy(),
            ValidTime::Unknown,
        )
        .await
        .unwrap();
    (auth, policy)
}
fn draft(auth: &Authority, policy: &PolicyVersion, statement: &str) -> RecordDraft {
    RecordDraft {
        label: statement.into(),
        scope: auth.scope.clone(),
        content: MemoryContent::Knowledge {
            statement: statement.into(),
            subject: None,
            predicate: None,
            uncertainty: vec!["Unverified outside inspected scope".into()],
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
            policy: policy.reference.clone(),
            action: PolicyAction::Retain,
            reason: "Useful for continuation".into(),
            constraints: vec![],
            required_evidence: vec![],
            expires_at: None,
        },
    }
}
fn relation(
    auth: &Authority,
    from: &MemoryVersion,
    to: &MemoryVersion,
    kind: RelationKind,
) -> RelationDraft {
    RelationDraft {
        from: from.reference.clone(),
        to: to.reference.clone(),
        kind,
        scope: auth.scope.clone(),
        basis: "Inspected source dependency".into(),
        evidential_status: EvidentialStatus::Observation,
        acceptance: Acceptance::Accepted,
        valid_time: ValidTime::Unknown,
    }
}
fn source(auth: &Authority, revision: &str, kind: SourceKind) -> SourceVersion {
    SourceVersion {
        reference: SourceRef {
            source_id: Uuid::now_v7(),
            revision: revision.into(),
        },
        label: "Pump schedule".into(),
        scope: auth.scope.clone(),
        kind,
        owner: "Project team".into(),
        acquired_at: Utc::now(),
        acquisition_method: "local import".into(),
        precedence: Some("issued".into()),
        snapshot_artifact: None,
    }
}
fn artifact(auth: &Authority, size: u32) -> ArtifactSpec {
    ArtifactSpec {
        label: "Inspection output".into(),
        scope: auth.scope.clone(),
        media_type: "text/plain".into(),
        expected_bytes: size,
        origin: Origin::Observed,
        retention_class: "project".into(),
        dependencies: vec![],
    }
}
fn time(day: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, day, 0, 0, 0).unwrap()
}

async fn temporal_records(store: &Store) {
    let (auth, policy) = setup(store).await;
    let mut c = draft(&auth, &policy, "Revision C: flow is 10");
    c.valid_time = ValidTime::Interval {
        from: Some(time(1)),
        to: Some(time(10)),
    };
    let c = store.create_memory(&auth, c).await.unwrap();
    let mut d = c.record.clone();
    d.label = "Revision D: corrected flow is 12".into();
    d.content = draft(&auth, &policy, &d.label).content;
    let d = store
        .revise_memory(&auth, &c.reference, d.clone())
        .await
        .unwrap();
    assert!(d.recorded.sequence > c.recorded.sequence);
    let old = store.memory(&auth, &c.reference).await.unwrap();
    assert_eq!(old.record.label, "Revision C: flow is 10");
    assert_eq!(old.recorded_until, Some(d.recorded.sequence));
    let at_c = store
        .memories(
            &auth,
            &MemoryQuery {
                recorded_as_of: Some(c.recorded.sequence),
                valid_at: Some(time(5)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(at_c[0].reference.revision.get(), 1);
    let at_d = store
        .memories(
            &auth,
            &MemoryQuery {
                recorded_as_of: Some(d.recorded.sequence),
                valid_at: Some(time(5)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(at_d[0].reference.revision.get(), 2);
    assert!(
        store
            .memories(
                &auth,
                &MemoryQuery {
                    valid_at: Some(time(10)),
                    ..Default::default()
                }
            )
            .await
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        store
            .revise_memory(&auth, &c.reference, d.record.clone())
            .await,
        Err(Error::Conflict)
    ));
    let unknown = store
        .create_memory(&auth, draft(&auth, &policy, "Unknown world time"))
        .await
        .unwrap();
    assert_eq!(
        store
            .memories(&auth, &MemoryQuery::default())
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        store
            .memories(
                &auth,
                &MemoryQuery {
                    valid_at: Some(time(5)),
                    ..Default::default()
                }
            )
            .await
            .unwrap()
            .len(),
        1
    );
    let before = store.position().await.unwrap();
    let mut invalid = draft(&auth, &policy, "");
    invalid.label = "Invalid".into();
    assert!(matches!(
        store
            .create_memories(
                &auth,
                vec![draft(&auth, &policy, "Must roll back"), invalid]
            )
            .await,
        Err(Error::Invalid(_))
    ));
    assert_eq!(store.position().await.unwrap(), before);
    assert_eq!(
        store
            .memories(&auth, &MemoryQuery::default())
            .await
            .unwrap()
            .len(),
        2
    );
    let mut hidden = unknown.record.clone();
    hidden.availability = Availability::Historical;
    store
        .revise_memory(&auth, &unknown.reference, hidden)
        .await
        .unwrap();
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
            .memories(
                &auth,
                &MemoryQuery {
                    include_inactive: true,
                    ..Default::default()
                }
            )
            .await
            .unwrap()
            .len(),
        2
    );
    let (first, second) = tokio::join!(
        store.revise_memory(&auth, &d.reference, d.record.clone()),
        store.revise_memory(&auth, &d.reference, d.record.clone())
    );
    assert!(matches!(
        (&first, &second),
        (Ok(_), Err(Error::Conflict)) | (Err(Error::Conflict), Ok(_))
    ));
    let p2 = store
        .revise_policy(
            &auth,
            &policy.reference,
            policy.policy.clone(),
            ValidTime::Unknown,
        )
        .await
        .unwrap();
    assert!(matches!(
        store
            .create_memory(&auth, draft(&auth, &policy, "Stale policy"))
            .await,
        Err(Error::Conflict)
    ));
    assert!(matches!(
        store
            .revise_policy(
                &auth,
                &policy.reference,
                policy.policy.clone(),
                ValidTime::Unknown
            )
            .await,
        Err(Error::Conflict)
    ));
    assert_eq!(
        store
            .policy(&auth, &policy.reference)
            .await
            .unwrap()
            .reference
            .revision
            .get(),
        1
    );
    store
        .create_memory(&auth, draft(&auth, &p2, "Current policy"))
        .await
        .unwrap();
}

async fn families_and_relations(store: &Store) {
    let (auth, policy) = setup(store).await;
    let base = store
        .create_memory(&auth, draft(&auth, &policy, "Common observation"))
        .await
        .unwrap();
    let mut episode = draft(&auth, &policy, "Episode");
    episode.content = MemoryContent::Episode {
        objective: "Inspect flow".into(),
        initial_conditions: vec![],
        observations: vec!["Flow shown as 12".into()],
        actions: vec!["Read schedule".into()],
        corrections: vec![],
        outcome: "Inspection finished".into(),
        verification: vec![],
        uncertainty: vec![],
    };
    episode.origin = Origin::Observed;
    episode.evidential_status = EvidentialStatus::Simulation;
    let mut procedure = draft(&auth, &policy, "Advisory method");
    procedure.content = MemoryContent::Procedure {
        purpose: "Check flow".into(),
        capabilities: vec![],
        applicability: vec!["Pump schedule".into()],
        exclusions: vec![],
        method: ProcedureForm::Advisory {
            steps: vec!["Compare issued schedules".into()],
            evidence_criteria: vec!["Revision and units are explicit".into()],
        },
        counterexamples: vec![],
        contract: None,
    };
    procedure.qualification = Qualification::Evaluated {
        evaluation_artifact: None,
        evaluator_profile: None,
        conditions: vec!["Synthetic schedule".into()],
        evidence: vec![base.reference.clone()],
    };
    let mut intention = draft(&auth, &policy, "Check next issue");
    intention.content = MemoryContent::Intention {
        purpose: "Review next schedule".into(),
        owner_id: auth.actor_id,
        trigger: "new source revision".into(),
        readiness: vec![],
        completion: vec!["Comparison recorded".into()],
        expires_at: None,
        notification_policy: "on_change".into(),
        recurrence: None,
        plan: None,
    };
    let versions = store
        .create_memories(&auth, vec![episode, procedure, intention])
        .await
        .unwrap();
    assert_eq!(
        versions
            .iter()
            .map(|v| v.record.content.family())
            .collect::<Vec<_>>(),
        ["episode", "procedure", "intention"]
    );
    let reopened = store.memory(&auth, &versions[0].reference).await.unwrap();
    assert_eq!(reopened.record.origin, Origin::Observed);
    assert_eq!(
        reopened.record.evidential_status,
        EvidentialStatus::Simulation
    );
    let mut left = draft(&auth, &policy, "Derived left");
    left.derived_from.push(base.reference.clone());
    let left = store.create_memory(&auth, left).await.unwrap();
    let mut right = draft(&auth, &policy, "Derived right");
    right.derived_from.push(base.reference.clone());
    let right = store.create_memory(&auth, right).await.unwrap();
    assert_eq!(
        store
            .relations(&auth, base.reference.memory_id, &MemoryQuery::default())
            .await
            .unwrap()
            .len(),
        2
    );
    assert!(matches!(
        store
            .create_relation(
                &auth,
                relation(&auth, &base, &left, RelationKind::DerivedFrom)
            )
            .await,
        Err(Error::Invalid(_))
    ));
    store
        .create_relation(&auth, relation(&auth, &base, &left, RelationKind::Supports))
        .await
        .unwrap();
    store
        .create_relation(&auth, relation(&auth, &left, &base, RelationKind::Supports))
        .await
        .unwrap();
    let conflict = store
        .create_relation(
            &auth,
            relation(&auth, &left, &right, RelationKind::ConflictsWith),
        )
        .await
        .unwrap();
    assert!(matches!(
        store
            .create_relation(
                &auth,
                relation(&auth, &right, &left, RelationKind::ConflictsWith)
            )
            .await,
        Err(Error::Conflict)
    ));
    let mut withdrawn = conflict.relation.clone();
    withdrawn.acceptance = Acceptance::Withdrawn;
    store
        .revise_relation(&auth, conflict.id, 1, withdrawn.clone())
        .await
        .unwrap();
    assert!(matches!(
        store
            .revise_relation(&auth, conflict.id, 1, withdrawn)
            .await,
        Err(Error::Conflict)
    ));
    let old = store
        .relations(
            &auth,
            left.reference.memory_id,
            &MemoryQuery {
                recorded_as_of: Some(conflict.recorded.sequence),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(old.iter().any(|r| r.id == conflict.id && r.revision == 1));
    assert!(
        !store
            .relations(&auth, left.reference.memory_id, &MemoryQuery::default())
            .await
            .unwrap()
            .iter()
            .any(|r| r.id == conflict.id)
    );
}

async fn scope_and_entities(store: &Store) {
    let (auth, policy) = setup(store).await;
    let record = store
        .create_memory(&auth, draft(&auth, &policy, "Project shared fact"))
        .await
        .unwrap();
    let other = Authority {
        tenant_id: auth.tenant_id,
        actor_id: auth.actor_id,
        scope: Scope {
            project_id: Some(Uuid::now_v7()),
            ..Default::default()
        },
    };
    assert!(matches!(
        store.memory(&other, &record.reference).await,
        Err(Error::NotFound)
    ));
    assert!(
        store
            .memories(&other, &MemoryQuery::default())
            .await
            .unwrap()
            .is_empty()
    );
    let tenant = authority();
    assert!(matches!(
        store.policy(&tenant, &policy.reference).await,
        Err(Error::NotFound)
    ));
    let narrow = Authority {
        tenant_id: auth.tenant_id,
        actor_id: auth.actor_id,
        scope: Scope {
            task_id: Some(Uuid::now_v7()),
            ..auth.scope.clone()
        },
    };
    assert!(store.memory(&narrow, &record.reference).await.is_ok());
    assert!(matches!(
        store
            .revise_policy(
                &narrow,
                &policy.reference,
                policy.policy.clone(),
                ValidTime::Unknown
            )
            .await,
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        store
            .revise_memory(&narrow, &record.reference, record.record.clone())
            .await,
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        store
            .revise_relation(
                &narrow,
                Uuid::now_v7(),
                1,
                relation(&auth, &record, &record, RelationKind::Supports)
            )
            .await,
        Err(Error::NotFound)
    ));
    let mut ids = vec![];
    for native in ["P-01", "Pump-01"] {
        let entity = Entity {
            id: Uuid::now_v7(),
            scope: auth.scope.clone(),
            provider: native.into(),
            native_id: native.into(),
            label: native.into(),
            aliases: vec!["Main pump".into()],
        };
        ids.push(store.create_entity(&auth, entity).await.unwrap().id);
    }
    assert_eq!(
        store.find_entities(&auth, "Main pump").await.unwrap().len(),
        2
    );
    assert!(
        store
            .find_entities(&other, "Main pump")
            .await
            .unwrap()
            .is_empty()
    );
    let link = EntityLink {
        id: Uuid::now_v7(),
        revision: 1,
        from: ids[0],
        to: ids[1],
        acceptance: Acceptance::Candidate,
        basis: "Matching schedule labels".into(),
    };
    let mut link = store.save_entity_link(&auth, link, None).await.unwrap();
    link.acceptance = Acceptance::Accepted;
    let link = store.save_entity_link(&auth, link, Some(1)).await.unwrap();
    assert_eq!(
        store.entity_link(&auth, link.id).await.unwrap().acceptance,
        Acceptance::Accepted
    );
    assert!(matches!(
        store.save_entity_link(&auth, link, Some(1)).await,
        Err(Error::Conflict)
    ));
    let mut scoped = draft(&auth, &policy, "Entity fact");
    scoped.scope.entity_ids.push(ids[0]);
    let scoped = store.create_memory(&auth, scoped).await.unwrap();
    assert_eq!(
        store
            .memories(
                &auth,
                &MemoryQuery {
                    entity_id: Some(ids[0]),
                    ..Default::default()
                }
            )
            .await
            .unwrap()[0]
            .reference
            .memory_id,
        scoped.reference.memory_id
    );
    let restricted = Authority {
        tenant_id: auth.tenant_id,
        actor_id: auth.actor_id,
        scope: Scope {
            entity_ids: vec![ids[1]],
            ..auth.scope.clone()
        },
    };
    assert!(matches!(
        store.memory(&restricted, &scoped.reference).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        store.entity(&restricted, ids[0]).await,
        Err(Error::NotFound)
    ));
    let matches = store.find_entities(&restricted, "Main pump").await.unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].id, ids[1]);

    let edge = store
        .create_relation(
            &auth,
            relation(&auth, &record, &scoped, RelationKind::Supports),
        )
        .await
        .unwrap();
    assert!(
        !store
            .relations(
                &restricted,
                record.reference.memory_id,
                &MemoryQuery::default()
            )
            .await
            .unwrap()
            .iter()
            .any(|r| r.id == edge.id)
    );
    let mut personal = draft(&auth, &policy, "Personal preference");
    personal.scope.user_id = Some(auth.actor_id);
    let personal = store.create_memory(&auth, personal).await.unwrap();
    let another_user = Authority {
        tenant_id: auth.tenant_id,
        actor_id: Uuid::now_v7(),
        scope: Scope {
            user_id: Some(Uuid::now_v7()),
            ..auth.scope.clone()
        },
    };
    assert!(matches!(
        store.memory(&another_user, &personal.reference).await,
        Err(Error::NotFound)
    ));
}

async fn source_revisions(store: &Store, root: &std::path::Path) {
    let (auth, policy) = setup(store).await;
    let artifacts = ArtifactService::local(store.clone(), root.join("sources"))
        .await
        .unwrap();
    let sources = SourceService::new(store.clone(), artifacts.clone(), 64 * 1024).unwrap();
    let path = root.join("schedule.txt");
    tokio::fs::write(&path, "Revision C\nFlow = 10\n")
        .await
        .unwrap();
    let c = sources
        .ingest_file(
            &auth,
            source(&auth, "C", SourceKind::Document),
            &DocumentAdapter,
            &path,
        )
        .await
        .unwrap();
    tokio::fs::write(&path, "Revision D\nFlow = 12\n")
        .await
        .unwrap();
    let mut d = c.clone();
    d.reference.revision = "D".into();
    let d = sources
        .ingest_file(&auth, d, &DocumentAdapter, &path)
        .await
        .unwrap();
    let loc = Locator::Document {
        start_line: 2,
        end_line: 2,
        page: None,
    };
    assert_eq!(
        sources
            .read(&auth, &c.reference, &loc, 100)
            .await
            .unwrap()
            .content,
        "Flow = 10"
    );
    assert_eq!(
        sources
            .read(&auth, &d.reference, &loc, 100)
            .await
            .unwrap()
            .content,
        "Flow = 12"
    );
    assert_eq!(
        store
            .source_versions(&auth, c.reference.source_id)
            .await
            .unwrap()
            .len(),
        2
    );
    let locator = store
        .create_source_locator(
            &auth,
            SourceLocator {
                id: Uuid::now_v7(),
                source: c.reference.clone(),
                locator: loc.clone(),
            },
        )
        .await
        .unwrap();
    let mut record = draft(&auth, &policy, "C says 10");
    record.source_locators.push(locator.id);
    record.scope.source_versions.push(c.reference.clone());
    let record = store.create_memory(&auth, record).await.unwrap();
    assert_eq!(
        sources
            .read_locator(&auth, record.record.source_locators[0], 100)
            .await
            .unwrap()
            .content,
        "Flow = 10"
    );
    let d_only = Authority {
        tenant_id: auth.tenant_id,
        actor_id: auth.actor_id,
        scope: Scope {
            source_versions: vec![d.reference.clone()],
            ..auth.scope.clone()
        },
    };
    assert!(matches!(
        store.memory(&d_only, &record.reference).await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        sources.read(&d_only, &c.reference, &loc, 100).await,
        Err(Error::NotFound)
    ));
    assert_eq!(
        store
            .source_versions(&d_only, c.reference.source_id)
            .await
            .unwrap()
            .len(),
        1
    );

    assert!(matches!(
        sources.read(&auth, &c.reference, &loc, 2).await,
        Err(Error::LimitExceeded)
    ));
    let unavailable = store
        .register_source_version(&auth, source(&auth, "missing", SourceKind::Document))
        .await
        .unwrap();
    assert!(matches!(
        sources.read(&auth, &unavailable.reference, &loc, 100).await,
        Err(Error::Unavailable)
    ));
    let table = sources
        .ingest(
            &auth,
            source(&auth, "1", SourceKind::Table),
            &TableAdapter,
            b"tag,description\nP-01,\"Pump, main\"\n",
        )
        .await
        .unwrap();
    assert_eq!(
        sources
            .read(
                &auth,
                &table.reference,
                &Locator::Table {
                    row: 1,
                    column: Some("description".into())
                },
                100
            )
            .await
            .unwrap()
            .content,
        "Pump, main"
    );
    let model = sources.ingest(&auth, source(&auth, "1", SourceKind::Model), &ModelAdapter, br#"{"entities":{"instance-1":{"properties":{"type":"pump-type","flow":12}},"pump-type":{"properties":{"design_flow":14}}}}"#).await.unwrap();
    assert_eq!(
        sources
            .read(
                &auth,
                &model.reference,
                &Locator::Model {
                    entity: "instance-1".into(),
                    property: "flow".into()
                },
                100
            )
            .await
            .unwrap()
            .content,
        12
    );
    assert!(matches!(
        sources
            .read(
                &auth,
                &model.reference,
                &Locator::Model {
                    entity: "instance-1".into(),
                    property: "design_flow".into()
                },
                100
            )
            .await,
        Err(Error::NotFound)
    ));
    let events = serde_json::json!({"schema_version":"memory-tool-events/1", "events":[{"event_id":"one", "observed_at":"2026-09-01T00:00:00Z", "kind":"tool_result", "origin":"agent_generated", "evidential_status":"simulation", "content":{"flow":12}}]});
    let events = sources
        .ingest(
            &auth,
            source(&auth, "1", SourceKind::ToolEvents),
            &ToolEventAdapter,
            &serde_json::to_vec(&events).unwrap(),
        )
        .await
        .unwrap();
    let read = sources
        .read(
            &auth,
            &events.reference,
            &Locator::Events { start: 1, end: 1 },
            1000,
        )
        .await
        .unwrap();
    assert_eq!(read.content[0]["evidential_status"], "simulation");
    assert!(
        ToolEventAdapter
            .normalize(br#"{"schema_version":"unknown","events":[]}"#)
            .is_err()
    );
    assert!(TableAdapter.normalize(b"a,a\n1,2").is_err());
    assert!(matches!(
        TableAdapter.read(
            br#"{"columns":["a","b"],"rows":[["one"]]}"#,
            &Locator::Table {
                row: 1,
                column: Some("b".into())
            }
        ),
        Err(Error::Invalid(_))
    ));
    assert!(DocumentAdapter.normalize(&[0xff]).is_err());
    let snapshot = c.snapshot_artifact.unwrap();
    let meta = artifacts.inspect(&auth, snapshot).await.unwrap();
    artifacts
        .revoke(&auth, snapshot, meta.revision)
        .await
        .unwrap();
    assert!(matches!(
        sources.read(&auth, &c.reference, &loc, 100).await,
        Err(Error::Unavailable)
    ));
    assert_eq!(
        sources
            .read(&auth, &d.reference, &loc, 100)
            .await
            .unwrap()
            .content,
        "Flow = 12"
    );
}

async fn artifact_lifecycle(store: &Store, root: &std::path::Path) {
    let (auth, _) = setup(store).await;
    let service = ArtifactService::local(store.clone(), root.join("artifacts"))
        .await
        .unwrap();
    let pending = service.allocate(&auth, artifact(&auth, 4)).await.unwrap();
    assert!(matches!(
        service.read(&auth, pending.id, 0..4).await,
        Err(Error::Unavailable)
    ));
    let mut upload = service.begin_upload(&auth, pending.id, 1).await.unwrap();
    upload.write(b"ab").await.unwrap();
    drop(upload);
    let retry = service.recover_upload(&auth, pending.id, 2).await.unwrap();
    assert_eq!(retry.state, ArtifactState::Pending);
    assert!(matches!(
        service.begin_upload(&auth, pending.id, 1).await,
        Err(Error::Conflict)
    ));
    let ready = service
        .upload(&auth, retry.id, retry.revision, b"abcd".as_slice())
        .await
        .unwrap();
    assert_eq!(
        service.read(&auth, ready.id, 1..3).await.unwrap().as_ref(),
        b"bc"
    );
    let mut output = Vec::new();
    assert_eq!(
        service.export(&auth, ready.id, &mut output).await.unwrap(),
        4
    );
    assert_eq!(output, b"abcd");
    let mut dependent = artifact(&auth, 3);
    dependent.dependencies.push(ready.id);
    let dependent = service.allocate(&auth, dependent).await.unwrap();
    let dependent = service
        .upload(&auth, dependent.id, 1, b"out".as_slice())
        .await
        .unwrap();
    service
        .revoke(&auth, ready.id, ready.revision)
        .await
        .unwrap();
    assert!(matches!(
        service.read(&auth, dependent.id, 0..3).await,
        Err(Error::Unavailable)
    ));
    assert!(matches!(
        service.revoke(&auth, ready.id, ready.revision).await,
        Err(Error::Conflict)
    ));
    let partial = service.allocate(&auth, artifact(&auth, 5)).await.unwrap();
    assert!(matches!(
        service
            .upload(&auth, partial.id, 1, b"abc".as_slice())
            .await,
        Err(Error::Invalid(_))
    ));
    assert_eq!(
        service
            .recover_upload(&auth, partial.id, 2)
            .await
            .unwrap()
            .state,
        ArtifactState::Pending
    );
    let large = vec![b'x'; 2 * 1024 * 1024 + 7];
    let allocated = service
        .allocate(&auth, artifact(&auth, large.len() as u32))
        .await
        .unwrap();
    let large_meta = service
        .upload(&auth, allocated.id, 1, large.as_slice())
        .await
        .unwrap();
    assert!(matches!(
        service
            .read(&auth, large_meta.id, 0..large_meta.spec.expected_bytes)
            .await,
        Err(Error::LimitExceeded)
    ));
    let mut export = Vec::new();
    service
        .export(&auth, large_meta.id, &mut export)
        .await
        .unwrap();
    assert_eq!(export.len(), large.len());
    let wrong = authority();
    assert!(matches!(
        service.inspect(&wrong, large_meta.id).await,
        Err(Error::NotFound)
    ));
    let narrow = Authority {
        tenant_id: auth.tenant_id,
        actor_id: auth.actor_id,
        scope: Scope {
            task_id: Some(Uuid::now_v7()),
            ..auth.scope.clone()
        },
    };
    assert!(matches!(
        service
            .revoke(&narrow, large_meta.id, large_meta.revision)
            .await,
        Err(Error::Forbidden)
    ));
}

async fn reopen_and_recover(url: &str, root: &std::path::Path) {
    let store = Store::connect(url).await.unwrap();
    let (auth, policy) = setup(&store).await;
    let memory = store
        .create_memory(&auth, draft(&auth, &policy, "Survives restart"))
        .await
        .unwrap();
    let service = ArtifactService::local(store.clone(), root.join("recovery"))
        .await
        .unwrap();
    let allocated = service.allocate(&auth, artifact(&auth, 4)).await.unwrap();
    let mut upload = service.begin_upload(&auth, allocated.id, 1).await.unwrap();
    upload.write(b"done").await.unwrap();
    let completion = upload.complete().await.unwrap();
    assert!(matches!(
        service.read(&auth, allocated.id, 0..4).await,
        Err(Error::Unavailable)
    ));
    store.close().await;
    let store = Store::connect(url).await.unwrap();
    let service = ArtifactService::local(store.clone(), root.join("recovery"))
        .await
        .unwrap();
    let recovered = service
        .recover_upload(&auth, allocated.id, 2)
        .await
        .unwrap();
    assert_eq!(recovered.state, ArtifactState::Ready);
    assert_eq!(
        service
            .read(&auth, recovered.id, 0..4)
            .await
            .unwrap()
            .as_ref(),
        b"done"
    );
    assert!(matches!(
        service.publish(&auth, completion).await,
        Err(Error::Conflict)
    ));
    assert_eq!(
        store
            .memory(&auth, &memory.reference)
            .await
            .unwrap()
            .record
            .label,
        "Survives restart"
    );
    store.close().await;
}

async fn suite(url: &str, root: &std::path::Path) {
    let store = Store::connect(url).await.unwrap();
    temporal_records(&store).await;
    late_world_change(&store).await;
    families_and_relations(&store).await;
    scope_and_entities(&store).await;
    source_revisions(&store, root).await;
    artifact_lifecycle(&store, root).await;
    lost_objects_and_attempts(&store, root).await;
    property_location(&store, root).await;
    store.close().await;
    reopen_and_recover(url, root).await;
}

#[tokio::test]
async fn sqlite_repository_contract() {
    let temp = TempDir::new().unwrap();
    suite(
        &format!(
            "sqlite://{}?mode=rwc",
            temp.path().join("memory.sqlite").display()
        ),
        temp.path(),
    )
    .await;
}
#[tokio::test]
#[ignore = "run with scripts/test-store.py using an isolated PostgreSQL cluster"]
async fn postgres_repository_contract() {
    let temp = TempDir::new().unwrap();
    suite(
        &std::env::var("MEMORY_TEST_POSTGRES_URL").expect("isolated test database URL"),
        temp.path(),
    )
    .await;
}

async fn migration_upgrade(url: &str) {
    use sqlx::Row;
    sqlx::any::install_default_drivers();
    let pool = sqlx::AnyPool::connect(url).await.unwrap();
    // Seed an earlier schema and its data, then open it with the current worker.
    for statement in include_str!("../migrations/001_records.sql")
        .split(';')
        .filter(|s| !s.trim().is_empty())
    {
        sqlx::query(statement).execute(&pool).await.unwrap();
    }
    sqlx::query("CREATE TABLE schema_migrations (version BIGINT PRIMARY KEY)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO schema_migrations VALUES (1)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("CREATE TABLE commit_clock (id BIGINT PRIMARY KEY, sequence BIGINT NOT NULL)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO commit_clock VALUES (1,17)")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let store = Store::connect(url).await.unwrap();
    assert_eq!(store.position().await.unwrap(), 17);
    let (auth, _) = setup(&store).await;
    let entity = Entity {
        id: Uuid::now_v7(),
        scope: auth.scope.clone(),
        provider: "model".into(),
        native_id: "W-101".into(),
        label: "Wall 101".into(),
        aliases: vec![],
    };
    store.create_entity(&auth, entity.clone()).await.unwrap();
    store.close().await;
    let store = Store::connect(url).await.unwrap();
    assert_eq!(
        store.entity(&auth, entity.id).await.unwrap().native_id,
        "W-101"
    );
    store.close().await;
    let pool = sqlx::AnyPool::connect(url).await.unwrap();
    let row = sqlx::query("SELECT MAX(version) AS version FROM schema_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(row.get::<i64, _>("version"), 11);
    sqlx::query("INSERT INTO schema_migrations VALUES (999)")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    assert!(matches!(Store::connect(url).await, Err(Error::Invalid(_))));
}

#[tokio::test]
async fn sqlite_migration_upgrade() {
    let temp = TempDir::new().unwrap();
    migration_upgrade(&format!(
        "sqlite://{}?mode=rwc",
        temp.path().join("upgrade.sqlite").display()
    ))
    .await;
}

#[tokio::test]
#[ignore = "run with scripts/test-store.py using an isolated PostgreSQL cluster"]
async fn postgres_migration_upgrade() {
    sqlx::any::install_default_drivers();
    let url = std::env::var("MEMORY_TEST_POSTGRES_URL").expect("isolated test database URL");
    let pool = sqlx::AnyPool::connect(&url).await.unwrap();
    let schema = format!("upgrade_{}", Uuid::now_v7().simple());
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let separator = if url.contains('?') { '&' } else { '?' };
    migration_upgrade(&format!("{url}{separator}options=-csearch_path%3D{schema}")).await;
}

async fn lost_objects_and_attempts(store: &Store, root: &std::path::Path) {
    let (auth, _) = setup(store).await;
    let root = root.join("lost-objects");
    let service = ArtifactService::local(store.clone(), &root).await.unwrap();
    let allocated = service.allocate(&auth, artifact(&auth, 4)).await.unwrap();
    let mut upload = service.begin_upload(&auth, allocated.id, 1).await.unwrap();
    upload.write(b"lost").await.unwrap();
    let stale_completion = upload.complete().await.unwrap();
    // Fault injection: completed storage disappears before database publication.
    let object_dir = root
        .join(auth.tenant_id.to_string())
        .join(allocated.id.to_string());
    for entry in std::fs::read_dir(&object_dir).unwrap() {
        std::fs::remove_file(entry.unwrap().path()).unwrap();
    }
    let retry = service
        .recover_upload(&auth, allocated.id, 2)
        .await
        .unwrap();
    let ready = service
        .upload(&auth, allocated.id, retry.revision, b"live".as_slice())
        .await
        .unwrap();
    assert!(matches!(
        service.publish(&auth, stale_completion).await,
        Err(Error::Conflict)
    ));
    assert_eq!(
        service.read(&auth, ready.id, 0..4).await.unwrap().as_ref(),
        b"live"
    );
    for entry in std::fs::read_dir(&object_dir).unwrap() {
        std::fs::remove_file(entry.unwrap().path()).unwrap();
    }
    assert!(matches!(
        service.read(&auth, ready.id, 0..4).await,
        Err(Error::Unavailable)
    ));
    assert!(matches!(
        service.export(&auth, ready.id, Vec::new()).await,
        Err(Error::Unavailable)
    ));
    let empty = service.allocate(&auth, artifact(&auth, 0)).await.unwrap();
    let empty = service
        .upload(&auth, empty.id, 1, b"".as_slice())
        .await
        .unwrap();
    assert!(
        service
            .read(&auth, empty.id, 0..0)
            .await
            .unwrap()
            .is_empty()
    );
    let object_dir = root
        .join(auth.tenant_id.to_string())
        .join(empty.id.to_string());
    for entry in std::fs::read_dir(object_dir).unwrap() {
        std::fs::remove_file(entry.unwrap().path()).unwrap();
    }
    assert!(matches!(
        service.read(&auth, empty.id, 0..0).await,
        Err(Error::Unavailable)
    ));
}

async fn property_location(store: &Store, root: &std::path::Path) {
    let world: serde_json::Value = serde_json::from_str(include_str!(
        "../../../evals/property-location/reference-world.json"
    ))
    .unwrap();
    let (auth, policy) = setup(store).await;
    let artifacts = ArtifactService::local(store.clone(), root.join("property-location"))
        .await
        .unwrap();
    let sources = SourceService::new(store.clone(), artifacts, 65536).unwrap();
    let mut reference = None;
    for revision in ["C", "D"] {
        let facts = &world["facts"][revision];
        let model = serde_json::json!({"entities": {
            facts["instance"].as_str().unwrap(): {"properties": facts["instance_values"]},
            facts["type"].as_str().unwrap(): {"properties": facts["type_values"]}
        }});
        let mut input = source(&auth, revision, SourceKind::Model);
        if let Some(id) = reference {
            input.reference.source_id = id;
        }
        let ingested = sources
            .ingest(
                &auth,
                input,
                &ModelAdapter,
                &serde_json::to_vec(&model).unwrap(),
            )
            .await
            .unwrap();
        reference = Some(ingested.reference.source_id);
        let (present, absent, value) = if revision == "C" {
            ("WT-12", "W-101", "120 min")
        } else {
            ("W-101", "WT-12", "90 min")
        };
        let loc = Locator::Model {
            entity: present.into(),
            property: "FireResistance".into(),
        };
        assert_eq!(
            sources
                .read(&auth, &ingested.reference, &loc, 100)
                .await
                .unwrap()
                .content,
            value
        );
        assert!(matches!(
            sources
                .read(
                    &auth,
                    &ingested.reference,
                    &Locator::Model {
                        entity: absent.into(),
                        property: "FireResistance".into()
                    },
                    100
                )
                .await,
            Err(Error::NotFound)
        ));
        let locator = store
            .create_source_locator(
                &auth,
                SourceLocator {
                    id: Uuid::now_v7(),
                    source: ingested.reference.clone(),
                    locator: loc,
                },
            )
            .await
            .unwrap();
        let mut finding = draft(
            &auth,
            &policy,
            &format!("{revision}: FireResistance on {present} = {value}"),
        );
        finding.scope.source_versions.push(ingested.reference);
        finding.source_locators.push(locator.id);
        store.create_memory(&auth, finding).await.unwrap();
    }
    let c_scope = Authority {
        tenant_id: auth.tenant_id,
        actor_id: auth.actor_id,
        scope: Scope {
            source_versions: vec![SourceRef {
                source_id: reference.unwrap(),
                revision: "C".into(),
            }],
            ..auth.scope.clone()
        },
    };
    let c = store
        .memories(&c_scope, &MemoryQuery::default())
        .await
        .unwrap();
    assert_eq!(c.len(), 1);
    assert!(c[0].record.label.contains("WT-12 = 120 min"));
    assert_eq!(
        sources
            .read_locator(&auth, c[0].record.source_locators[0], 100)
            .await
            .unwrap()
            .content,
        "120 min"
    );
}

async fn late_world_change(store: &Store) {
    let (auth, policy) = setup(store).await;
    let mut c = draft(&auth, &policy, "C: current flow is 10");
    c.valid_time = ValidTime::Interval {
        from: Some(time(1)),
        to: None,
    };
    let c = store.create_memory(&auth, c).await.unwrap();
    let mut closed = c.record.clone();
    closed.valid_time = ValidTime::Interval {
        from: Some(time(1)),
        to: Some(time(10)),
    };
    let mut d = draft(&auth, &policy, "D: flow changes to 12 on the 10th");
    d.valid_time = ValidTime::Interval {
        from: Some(time(10)),
        to: None,
    };
    let mut invalid = d.clone();
    invalid.valid_time = ValidTime::Interval {
        from: Some(time(10)),
        to: Some(time(9)),
    };
    let before = store.position().await.unwrap();
    assert!(matches!(
        store
            .apply_memories(
                &auth,
                vec![
                    MemoryChange::Revise {
                        expected: c.reference.clone(),
                        record: closed.clone()
                    },
                    MemoryChange::Create(invalid)
                ]
            )
            .await,
        Err(Error::Invalid(_))
    ));
    assert_eq!(store.position().await.unwrap(), before);
    assert!(
        store
            .memory(&auth, &c.reference)
            .await
            .unwrap()
            .recorded_until
            .is_none()
    );
    let committed = store
        .apply_memories(
            &auth,
            vec![
                MemoryChange::Revise {
                    expected: c.reference.clone(),
                    record: closed,
                },
                MemoryChange::Create(d),
            ],
        )
        .await
        .unwrap();
    assert_eq!(
        committed[0].recorded.sequence,
        committed[1].recorded.sequence
    );
    store
        .create_relation(
            &auth,
            relation(
                &auth,
                &committed[1],
                &committed[0],
                RelationKind::Supersedes,
            ),
        )
        .await
        .unwrap();
    let prior = store
        .memories(
            &auth,
            &MemoryQuery {
                recorded_as_of: Some(c.recorded.sequence),
                valid_at: Some(time(10)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(prior.len(), 1);
    assert_eq!(prior[0].record.label, c.record.label);
    let current = store
        .memories(
            &auth,
            &MemoryQuery {
                valid_at: Some(time(10)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(current.len(), 1);
    assert_eq!(
        current[0].reference.memory_id,
        committed[1].reference.memory_id
    );
    let earlier = store
        .memories(
            &auth,
            &MemoryQuery {
                valid_at: Some(time(9)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(earlier.len(), 1);
    assert_eq!(earlier[0].reference.memory_id, c.reference.memory_id);
}
