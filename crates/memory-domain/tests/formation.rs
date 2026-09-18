use memory_domain::{contracts::*, formation::*, records::*, sources::*};
use uuid::Uuid;

#[test]
fn explicit_user_claims_do_not_become_shared_preferences_under_a_broad_grant() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../evals/formation/capture.json")).unwrap();
    let event: ToolEvent = serde_json::from_value(fixture["events"][4].clone()).unwrap();
    let input: FormationInput = serde_json::from_value(event.content.clone()).unwrap();
    let actor = input.actor_id;
    let source = SourceRef {
        source_id: Uuid::now_v7(),
        revision: "C".into(),
    };
    let entry = FormationEntry {
        position: 1,
        event,
        input,
        locator: SourceLocator {
            id: Uuid::now_v7(),
            source: source.clone(),
            locator: Locator::Events { start: 1, end: 1 },
        },
        native_locators: vec![],
        duplicate_records: None,
        previous_deferral: None,
    };
    let window = FormationWindow {
        id: Uuid::now_v7(),
        job_id: Uuid::now_v7(),
        source,
        operation: "capture".into(),
        policy: ConfigRef {
            id: Uuid::now_v7(),
            revision: 1.try_into().unwrap(),
            label: "Test".into(),
        },
        scope: Scope::default(),
        after: 0,
        through: 1,
        cutoff: entry.event.observed_at,
        entries: vec![entry.clone()],
        remaining: false,
    };
    let decision = PolicyDecision {
        policy: window.policy.clone(),
        action: PolicyAction::Retain,
        reason: "Supported attributed contribution".into(),
        constraints: vec![],
        required_evidence: vec![],
        expires_at: None,
    };
    let record = entry.record(&window, decision);
    assert_eq!(
        record.scope.user_id, actor,
        "A broad service grant must not make an individual's preference shared"
    );
    assert_eq!(
        record.evidential_status,
        EvidentialStatus::AttributedStatement
    );
}
