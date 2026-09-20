//! ABOUTME: Rechecks a Jev-picked next operation before the `decide` tool acts on it: the job must still
//! ABOUTME: be running, the picked target current, the operation known, and the budget must have headroom.
use crate::Host;
use memory_domain::{
    contracts::Authority,
    coordination::{Fence, Resources},
    decision::{DecisionChoice, DecisionTarget},
};
use memory_store::{Error, Result};

/// The generic operation kinds the `decide` tool's candidate set may offer.
const KNOWN_OPERATIONS: &[&str] = &[
    "continue",
    "inspect_further",
    "apply_procedure",
    "retrieve_more",
    "investigate_conflict",
    "return_unresolved",
];

/// A conservative token estimate per operation kind, checked against remaining budget headroom before
/// the recheck admits the pick; the operation's own work still reserves what it actually spends.
fn cost_estimate(operation: &str) -> Resources {
    let tokens = match operation {
        "continue" | "return_unresolved" => 0,
        "inspect_further" => 4_000,
        "apply_procedure" | "retrieve_more" => 12_000,
        "investigate_conflict" => 24_000,
        _ => 24_000,
    };
    Resources {
        tokens,
        ..Resources::default()
    }
}

impl Host {
    pub(crate) async fn check_decision(
        &self,
        auth: &Authority,
        fence: &Fence,
        choice: DecisionChoice,
    ) -> Result<(bool, Vec<String>)> {
        let job = self.store.assigned_job(auth, fence).await?;
        let mut reasons = Vec::new();
        if job.cancel_requested || job.deadline <= chrono::Utc::now() {
            reasons.push("job_not_running".to_string());
        }
        if !KNOWN_OPERATIONS.contains(&choice.operation.as_str()) {
            reasons.push("unknown_operation".to_string());
        }
        let narrow = Authority {
            scope: job.spec.brief.scope.clone(),
            ..auth.clone()
        };
        match &choice.target {
            None | Some(DecisionTarget::None) => {}
            Some(DecisionTarget::Memory { reference }) => {
                match self
                    .store
                    .current_memory(&narrow, reference.memory_id)
                    .await
                {
                    Ok(current) => {
                        if current.reference.revision != reference.revision
                            || !job.spec.brief.scope.permits(&current.record.scope)
                        {
                            reasons.push("stale".to_string());
                        }
                    }
                    Err(Error::NotFound | Error::Unavailable) => reasons.push("stale".to_string()),
                    Err(e) => return Err(e),
                }
            }
            Some(DecisionTarget::Artifact { artifact_id }) => {
                match self.artifacts.inspect(&narrow, *artifact_id).await {
                    Ok(artifact) => {
                        if !job.spec.brief.scope.permits(&artifact.spec.scope) {
                            reasons.push("stale".to_string());
                        }
                    }
                    Err(Error::NotFound | Error::Unavailable) => reasons.push("stale".to_string()),
                    Err(e) => return Err(e),
                }
            }
        }
        let usage = self
            .store
            .budget_usage(auth, job.spec.brief.limits.root_budget_id)
            .await?;
        let estimate = cost_estimate(&choice.operation);
        let projected = usage
            .committed
            .checked_add(usage.unresolved)
            .and_then(|v| v.checked_add(estimate));
        if !projected
            .map(|v| v.fits(usage.budget.limit))
            .unwrap_or(false)
        {
            reasons.push("budget_exceeded".to_string());
        }
        Ok((reasons.is_empty(), reasons))
    }
}
