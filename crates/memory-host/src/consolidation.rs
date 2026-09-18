use crate::Host;
use memory_domain::{
    consolidation::*, contracts::*, coordination::Fence, judgement::*, records::*,
};
use memory_store::{Error, Result};
use std::collections::BTreeMap;
use uuid::Uuid;

impl Host {
    pub(crate) async fn consolidation_window(
        &self,
        auth: &Authority,
        permit: &Fence,
        id: Uuid,
        selection: CohortSelection,
    ) -> Result<ConsolidationWindow> {
        let window = self
            .store
            .consolidation_window(auth, permit, id, selection)
            .await?;
        self.publish_work_artifact(
            auth,
            permit,
            id,
            "Consolidation cohort and source groups".into(),
            serde_json::to_string(&window)?,
            vec![],
        )
        .await?;
        Ok(window)
    }
    pub(crate) async fn read_consolidation_artifact<T: serde::de::DeserializeOwned>(
        &self,
        auth: &Authority,
        permit: &Fence,
        id: Uuid,
    ) -> Result<T> {
        self.store.input_artifact(auth, permit, id).await?;
        let artifact = self.artifacts.inspect(auth, id).await?;
        if artifact.spec.expected_bytes > 65536 {
            return Err(Error::LimitExceeded);
        }
        Ok(serde_json::from_slice(
            &self
                .artifacts
                .read(auth, id, 0..artifact.spec.expected_bytes)
                .await?,
        )?)
    }
    pub(crate) async fn review_consolidation(
        &self,
        auth: &Authority,
        permit: &Fence,
        id: Uuid,
        proposal: ConsolidationProposal,
    ) -> Result<ConsolidationReview> {
        proposal.validate().map_err(Error::Invalid)?;
        let window: ConsolidationWindow = self
            .read_consolidation_artifact(auth, permit, proposal.window_id)
            .await?;
        // Read the host-owned cohort receipt, rather than trusting a caller-published lookalike.
        let window = self
            .store
            .consolidation_window(auth, permit, window.id, window.selection)
            .await?;
        self.store.check_cohort(auth, &window).await?;
        if !window.scope.permits(&proposal.scope)
            || window
                .cases
                .iter()
                .any(|c| !c.record.scope.permits(&proposal.scope))
        {
            return Err(Error::Invalid("Scope extension requires separate transfer evidence; keep this synthesis within every source scope".into()));
        }
        for clause in &proposal.clauses {
            if clause
                .support
                .iter()
                .chain(&clause.exceptions)
                .any(|r| !window.cases.iter().any(|c| c.reference == *r))
            {
                return Err(Error::Forbidden);
            }
        }
        if let Some(MemoryContent::Procedure {
            method,
            contract: Some(contract),
            capabilities,
            counterexamples,
            ..
        }) = &proposal.procedure
        {
            let job = self.store.assigned_job(auth, permit).await?;
            if capabilities
                .iter()
                .any(|c| !job.spec.brief.capabilities.tools.contains(c))
                || counterexamples
                    .iter()
                    .any(|r| !window.cases.iter().any(|c| c.reference == *r))
            {
                return Err(Error::Forbidden);
            }
            if window
                .exceptions
                .iter()
                .any(|r| !counterexamples.contains(r))
            {
                return Err(Error::Invalid(
                    "Procedure must preserve the cohort counterexamples".into(),
                ));
            }
            if let ProcedureForm::Executable { artifact_id, .. } = method {
                self.store
                    .input_artifact(auth, permit, *artifact_id)
                    .await?;
            }
            if contract.recognition.len() > 8 {
                return Err(Error::LimitExceeded);
            }
            let mut ids = std::collections::BTreeSet::new();
            for c in &contract.recognition {
                if !ids.insert(&c.definition.id) {
                    return Err(Error::Invalid(
                        "Recognition criterion IDs must be unique".into(),
                    ));
                }
                c.definition.validate().map_err(Error::Invalid)?;
                if c.model.trim().is_empty()
                    || c.model_release.trim().is_empty()
                    || c.policy != window.policy
                    || c.definition
                        .permitted_uses
                        .iter()
                        .any(|u| u != "investigation")
                {
                    return Err(Error::Invalid("Recognition candidates need model/policy versions and investigative use only".into()));
                }
            }
        }
        let review = ConsolidationReview {
            id,
            window,
            proposal,
        };
        self.store
            .save_consolidation_review(auth, permit, &review)
            .await?;
        self.publish_work_artifact(
            auth,
            permit,
            id,
            "Conditional synthesis for review".into(),
            serde_json::to_string(&review)?,
            vec![review.window.id],
        )
        .await?;
        Ok(review)
    }
    async fn consolidation_answers(
        &self,
        auth: &Authority,
        permit: &Fence,
        review: &ConsolidationReview,
        id: Option<&Uuid>,
        family: &str,
        fields: BTreeMap<String, String>,
    ) -> Result<BTreeMap<String, JudgementAnswer>> {
        let Some(id) = id else {
            return Ok(BTreeMap::new());
        };
        let decision = self.store.judgement_decision(auth, permit, *id).await?;
        let packet = self.read_judgement_packet(auth, decision.packet_id).await?;
        self.store
            .check_judgement_packet(auth, permit, &packet)
            .await?;
        let catalogue: Vec<JudgementDefinition> = serde_json::from_str(include_str!(
            "../../../packages/judgement/src/catalogue-data.json"
        ))?;
        let definition = catalogue
            .iter()
            .find(|d| d.id == family)
            .ok_or(Error::Forbidden)?;
        if packet.local_check_id.is_some()
            || packet.questions.len() != 1
            || serde_json::to_value(&packet.questions[0].definition)?
                != serde_json::to_value(definition)?
            || packet.evidence.len() != fields.len()
        {
            return Err(Error::Forbidden);
        }
        let content = serde_json::to_value(review)?;
        for (name, pointer) in fields {
            let item = packet
                .evidence
                .iter()
                .find(|e| e.name == name)
                .ok_or(Error::Forbidden)?;
            if item.artifact_id != review.id
                || item.pointer != pointer
                || content.pointer(&pointer) != Some(&item.content)
            {
                return Err(Error::Forbidden);
            }
        }
        let Some(id) = decision.selected_assessment else {
            return Ok(BTreeMap::new());
        };
        let assessment = self.store.assessment(auth, id).await?;
        assessment.validate(&packet).map_err(Error::Invalid)?;
        Ok(assessment.answers)
    }
    pub(crate) async fn commit_consolidation(
        &self,
        auth: &Authority,
        permit: &Fence,
        request: ConsolidationCommit,
    ) -> Result<ConsolidationResult> {
        let review = self
            .store
            .consolidation_review(auth, permit, request.review_id)
            .await?;
        let w = &review.window;
        let mut reasons = w.unexamined.clone();
        let independent = w
            .source_groups
            .iter()
            .filter(|g| g.members.iter().all(|r| !w.unknown_lineage.contains(r)))
            .count() as u16;
        let policy = self.store.policy(auth, &w.policy).await?;
        match &policy.policy.consolidation {
            Some(rules) if independent >= rules.min_independent_sources => (),
            Some(_) => reasons.push("Insufficient independently sourced evidence".into()),
            None => reasons.push("No consolidation adoption policy is configured".into()),
        }
        for exception in &w.exceptions {
            if !review
                .proposal
                .clauses
                .iter()
                .any(|c| c.exceptions.contains(exception))
            {
                reasons.push(format!("Exception is not addressed: {}", exception.label));
            }
        }
        let cohort = self
            .consolidation_answers(
                auth,
                permit,
                &review,
                request.decisions.get("J11"),
                "J11",
                BTreeMap::from([
                    ("cases".into(), "/window/cases".into()),
                    ("source_lineage".into(), "/window/source_groups".into()),
                ]),
            )
            .await?;
        if answer(&cohort, "J11.comparable") != Some("yes")
            || !matches!(answer(&cohort, "J11.duplicate"), Some("yes" | "no"))
            || answer(&cohort, "J11.counterexample").is_none_or(|s| !["yes", "no"].contains(&s))
        {
            reasons.push("Cohort comparability or dependence remains unresolved".into());
        }
        if answer(&cohort, "J11.counterexample") == Some("yes")
            && review
                .proposal
                .clauses
                .iter()
                .all(|c| c.exceptions.is_empty())
        {
            reasons
                .push("A semantic counterexample needs an explicit conditional treatment".into());
        }
        if answer(&cohort, "J11.duplicate") == Some("yes")
            && !w.source_groups.iter().any(|g| g.members.len() > 1)
        {
            reasons.push("Possible common-source dependence needs investigation".into());
        }
        // The whole proposal is checked too: a faithful clause cannot license an unrelated method.
        for (key, pointer) in std::iter::once(("J12/proposal".to_string(), "/proposal".to_string()))
            .chain(
                review
                    .proposal
                    .clauses
                    .iter()
                    .enumerate()
                    .map(|(i, _)| (format!("J12/{i}"), format!("/proposal/clauses/{i}"))),
            )
        {
            let answers = self
                .consolidation_answers(
                    auth,
                    permit,
                    &review,
                    request.decisions.get(&key),
                    "J12",
                    BTreeMap::from([
                        ("clause".into(), pointer),
                        ("source_cases".into(), "/window/cases".into()),
                    ]),
                )
                .await?;
            if ["conditions", "uncertainty", "scope"]
                .iter()
                .any(|k| answer(&answers, &format!("J12.{k}")) != Some("yes"))
            {
                reasons.push(format!(
                    "Conditions, uncertainty or scope need repair: {key}"
                ));
            }
        }
        let result = ConsolidationResult {
            review_id: review.id,
            record: None,
            deferred: reasons,
            independent_sources: independent,
            coverage: Coverage {
                examined: w.cases.iter().map(|c| c.reference.label.clone()).collect(),
                unexamined: w.unexamined.clone(),
            },
        };
        self.store
            .commit_consolidation(auth, permit, &review, result)
            .await
    }
}
fn answer<'a>(answers: &'a BTreeMap<String, JudgementAnswer>, key: &str) -> Option<&'a str> {
    match answers.get(key) {
        Some(JudgementAnswer::Choice { choice, .. }) => Some(choice),
        _ => None,
    }
}
