use chrono::{DateTime, Utc};
use memory_domain::contracts::*;
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
use uuid::Uuid;

fn fixture(directory: &str, name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../evals/property-location/{directory}/{name}.json"
    ));
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn command() -> Command<WorkBrief> {
    serde_json::from_value(fixture("inputs", "before-correction")["command"].clone()).unwrap()
}

fn result(name: &str) -> WorkResult {
    serde_json::from_value(fixture("results", name)).unwrap()
}

#[test]
fn reference_checkpoints_are_valid_contracts() {
    for name in ["before-correction", "after-correction", "revision-d"] {
        let input = fixture("inputs", name);
        let command: Command<WorkBrief> = serde_json::from_value(input["command"].clone()).unwrap();
        command.validate().unwrap();
        result(name).validate().unwrap();
        for evidence in input["evidence"].as_array().unwrap() {
            let observed: DateTime<Utc> =
                serde_json::from_value(evidence["observed_at"].clone()).unwrap();
            assert!(observed <= command.payload.evidence_cutoff);
        }
    }
}

#[test]
fn authority_rejects_cross_tenant_actor_and_scope_changes() {
    let original = command();
    let authority = Authority {
        tenant_id: original.tenant_id,
        actor_id: original.actor_id,
        scope: original.scope.clone(),
    };
    let now = original.payload.evidence_cutoff;
    original.check_authority(&authority, now).unwrap();
    for field in [
        "tenant", "actor", "project", "task", "entity", "source", "revision",
    ] {
        let mut c = original.clone();
        match field {
            "tenant" => c.tenant_id = Uuid::now_v7(),
            "actor" => c.actor_id = Uuid::now_v7(),
            "project" => c.scope.project_id = None,
            "task" => c.scope.task_id = Some(Uuid::now_v7()),
            "entity" => c.scope.entity_ids.clear(),
            "source" => c.scope.source_versions.clear(),
            _ => c.scope.source_versions[0].revision = "D".into(),
        }
        assert_eq!(
            c.check_authority(&authority, now).unwrap_err().code,
            ReasonCode::ForbiddenScope,
            "{field}"
        );
    }
}

#[test]
fn broader_grant_can_authorize_a_narrower_request() {
    assert!(Scope::default().permits(&command().scope));
    assert!(!command().scope.permits(&Scope::default()));
}

#[test]
fn deadline_is_an_exclusive_boundary() {
    let c = command();
    let authority = Authority {
        tenant_id: c.tenant_id,
        actor_id: c.actor_id,
        scope: c.scope.clone(),
    };
    assert_eq!(
        c.check_authority(&authority, c.deadline).unwrap_err().code,
        ReasonCode::DeadlineExceeded
    );
}

#[test]
fn nested_brief_cannot_broaden_the_command() {
    let mut c = command();
    c.payload.scope.project_id = None;
    assert!(c.validate().is_err());
    let mut c = command();
    c.payload.capabilities.sources[0].revision = "D".into();
    assert!(c.validate().is_err());
    let mut c = command();
    c.payload.limits.root_budget_id = Uuid::now_v7();
    assert!(c.validate().is_err());
}

#[test]
fn partial_work_must_identify_remaining_work() {
    let mut r = result("before-correction");
    r.unresolved_work.clear();
    assert!(r.validate().is_err());
}

#[test]
fn completion_cannot_hide_coverage_or_unknown_effects() {
    let mut r = result("before-correction");
    r.status = WorkStatus::Complete;
    assert!(r.validate().is_err());
    let mut r = result("after-correction");
    r.known_effects.push(KnownEffect {
        effect_id: Uuid::now_v7(),
        status: EffectStatus::Unknown,
        description: "External write outcome is unknown".into(),
    });
    assert!(r.validate().is_err());
}

#[test]
fn findings_cannot_claim_unexamined_sources_or_broader_applicability() {
    let mut r = result("after-correction");
    r.findings[0].sources[0].revision = "D".into();
    assert!(r.validate().is_err());
    let mut r = result("after-correction");
    r.findings[0].applicability.source_versions.clear();
    assert!(r.validate().is_err());
    let mut r = result("after-correction");
    r.inputs.sources[0].revision = "D".into();
    assert!(r.validate().is_err());
    let mut r = result("after-correction");
    r.findings[0].supporting_memories.push(MemoryRef {
        memory_id: Uuid::now_v7(),
        revision: 1.try_into().unwrap(),
        label: "Not inspected".into(),
    });
    assert!(r.validate().is_err());
}

#[test]
fn revision_and_numeric_ranges_are_safe_for_json_clients() {
    let mut value = fixture("inputs", "before-correction")["command"].clone();
    for invalid in [json!(0), json!(-1), json!(1.5), json!(4294967296_u64)] {
        value["payload"]["policy"]["revision"] = invalid;
        assert!(serde_json::from_value::<Command<WorkBrief>>(value.clone()).is_err());
    }
    value["payload"]["policy"]["revision"] = json!(4294967295_u32);
    assert!(serde_json::from_value::<Command<WorkBrief>>(value).is_ok());
}

#[test]
fn evidence_status_and_origin_remain_independent() {
    let r = result("before-correction");
    assert_eq!(
        r.findings[0].evidential_status,
        EvidentialStatus::Observation
    );
    assert_eq!(r.findings[1].evidential_status, EvidentialStatus::Inference);
    assert_eq!(r.findings[1].origin, Origin::AgentGenerated);
    assert_eq!(result("revision-d").status, WorkStatus::Blocked);
}
