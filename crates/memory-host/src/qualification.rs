use crate::Host;
use chrono::{Duration, Utc};
use memory_domain::{consolidation::*, contracts::*, coordination::*, records::*};
use memory_store::{Error, Result};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

impl Host {
    pub(crate) async fn start_qualification(
        &self,
        auth: &Authority,
        permit: &Fence,
        request_id: Uuid,
        review_id: Uuid,
        suite_id: Uuid,
    ) -> Result<Job> {
        let parent = self.store.assigned_job(auth, permit).await?;
        let review = self
            .store
            .consolidation_review(auth, permit, review_id)
            .await?;
        let candidate = self
            .store
            .consolidation_result(auth, permit, review_id)
            .await?
            .record
            .ok_or(Error::Unavailable)?;
        self.store
            .current_consolidation_memory(auth, &candidate.reference)
            .await?;
        if !matches!(candidate.record.content, MemoryContent::Procedure { .. })
            || !matches!(candidate.record.qualification, Qualification::Candidate)
            || !parent.spec.brief.inputs.artifacts.contains(&suite_id)
        {
            return Err(Error::Forbidden);
        }
        let suite: QualificationSuite = self
            .read_consolidation_artifact(auth, permit, suite_id)
            .await?;
        let rules = self
            .store
            .policy(auth, &review.window.policy)
            .await?
            .policy
            .consolidation
            .ok_or(Error::Forbidden)?;
        check_suite(&suite, &review.window, &rules)?;
        if rules.evaluator_profile != parent.spec.brief.profile {
            return Err(Error::Forbidden);
        }
        let input = QualificationInput {
            review,
            candidate,
            suite_id,
        };
        self.publish_work_artifact(
            auth,
            permit,
            request_id,
            "Procedure qualification inputs".into(),
            serde_json::to_string(&input)?,
            vec![review_id, suite_id],
        )
        .await?;
        let mut brief = parent.spec.brief.clone();
        brief.process = Process::Evaluation;
        brief.purpose = format!("Qualify procedure: {}", input.candidate.reference.label);
        brief.inputs = WorkInputs {
            sources: vec![],
            memories: vec![],
            artifacts: vec![request_id, suite_id],
        };
        // Executable code is delegated as its governed artifact, never as a new host privilege.
        if let MemoryContent::Procedure {
            method: ProcedureForm::Executable { artifact_id, .. },
            ..
        } = input.candidate.record.content
        {
            brief.inputs.artifacts.push(artifact_id);
        }
        brief.output_criteria = vec!["Compare all held-out tasks across episodes, summary and procedure; publish observed answers and evidence".into()];
        self.store
            .spawn_child(
                auth,
                permit,
                request_id,
                brief,
                parent.deadline - Duration::seconds(2),
                None,
            )
            .await
    }
    pub(crate) async fn adopt_procedure(
        &self,
        auth: &Authority,
        permit: &Fence,
        evaluation_job: Uuid,
    ) -> Result<AdoptionResult> {
        let child = self
            .store
            .child_jobs(auth, permit, &[evaluation_job])
            .await?
            .remove(0);
        if child.spec.brief.process != Process::Evaluation
            || child.spec.parent_id != Some(permit.job_id)
        {
            return Err(Error::Forbidden);
        }
        if let Some(prior) = self.store.adoption(auth, evaluation_job).await? {
            return Ok(prior);
        }
        if child.state != JobState::Completed
            || child.cancel_requested
            || child.spec.retain_until <= Utc::now()
        {
            return Err(Error::Unavailable);
        }
        let input_id = *child
            .spec
            .brief
            .inputs
            .artifacts
            .first()
            .ok_or(Error::Invalid("Evaluation has no input manifest".into()))?;
        let input: QualificationInput = self
            .read_consolidation_artifact(auth, permit, input_id)
            .await?;
        let review = self
            .store
            .consolidation_review(auth, permit, input.review.id)
            .await?;
        let result = self
            .store
            .consolidation_result(auth, permit, review.id)
            .await?;
        if serde_json::to_value(&review)? != serde_json::to_value(&input.review)?
            || serde_json::to_value(&result.record)?
                != serde_json::to_value(Some(&input.candidate))?
        {
            return Err(Error::Forbidden);
        }
        let suite: QualificationSuite = self
            .read_consolidation_artifact(auth, permit, input.suite_id)
            .await?;
        let rules = self
            .store
            .policy(auth, &review.window.policy)
            .await?
            .policy
            .consolidation
            .ok_or(Error::Forbidden)?;
        check_suite(&suite, &review.window, &rules)?;
        let work = child.result.as_ref().ok_or(Error::Unavailable)?;
        let [report_id] = work.child_outputs.as_slice() else {
            return Err(Error::Invalid(
                "Evaluation must publish one comparison report".into(),
            ));
        };
        let report_id = *report_id;
        self.store
            .qualification_evidence(auth, evaluation_job, report_id)
            .await?;
        let artifact = self.artifacts.inspect(auth, report_id).await?;
        if artifact.spec.expected_bytes > 65536 {
            return Err(Error::LimitExceeded);
        }
        let report: QualificationReport = serde_json::from_slice(
            &self
                .artifacts
                .read(auth, report_id, 0..artifact.spec.expected_bytes)
                .await?,
        )?;
        if report.review_id != review.id
            || report.candidate != input.candidate.reference
            || report.suite_id != input.suite_id
            || report.evaluator_profile != rules.evaluator_profile
            || child.spec.brief.profile != rules.evaluator_profile
        {
            return Err(Error::Forbidden);
        }
        let mut reasons = report.unresolved.clone();
        if !work.unresolved_work.is_empty() || !work.coverage.unexamined.is_empty() {
            reasons.push("Evaluation has incomplete coverage".into());
        }
        let mut rates = BTreeMap::new();
        let mut cost = 0_u64;
        let mut evidence_ids = BTreeSet::new();
        for arm in [
            QualificationArm::Episodes,
            QualificationArm::Summary,
            QualificationArm::Procedure,
        ] {
            let mut successes = 0;
            for case in &suite.cases {
                let trials: Vec<_> = report
                    .trials
                    .iter()
                    .filter(|t| t.case_id == case.id && t.arm == arm)
                    .collect();
                if trials.len() != 1 {
                    return Err(Error::Invalid(
                        "Each held-out case needs exactly one trial in every comparison arm".into(),
                    ));
                }
                let trial = trials[0];
                if !evidence_ids.insert(trial.evidence_artifact) {
                    return Err(Error::Invalid(
                        "Trials require separate observed evidence".into(),
                    ));
                }
                self.store
                    .qualification_evidence(auth, evaluation_job, trial.evidence_artifact)
                    .await?;
                let artifact = self
                    .artifacts
                    .inspect(auth, trial.evidence_artifact)
                    .await?;
                if artifact.spec.expected_bytes > 65536 {
                    return Err(Error::LimitExceeded);
                }
                let observed: serde_json::Value = serde_json::from_slice(
                    &self
                        .artifacts
                        .read(auth, artifact.id, 0..artifact.spec.expected_bytes)
                        .await?,
                )?;
                if observed.get("answer") != Some(&trial.answer)
                    || observed.get("cost_microunits")
                        != Some(&serde_json::to_value(trial.cost_microunits)?)
                    || observed.get("recognition")
                        != Some(&serde_json::to_value(&trial.recognition)?)
                {
                    return Err(Error::Invalid(
                        "Reported answer, cost or recognition differs from observed trial evidence"
                            .into(),
                    ));
                }
                successes += usize::from(trial.answer == case.expected);
                cost += u64::from(trial.cost_microunits);
            }
            let key = match arm {
                QualificationArm::Episodes => "episodes",
                QualificationArm::Summary => "summary",
                QualificationArm::Procedure => "procedure",
            };
            rates.insert(key.to_string(), successes as f64 / suite.cases.len() as f64);
        }
        if report.trials.len() != suite.cases.len() * 3 {
            return Err(Error::Invalid("Report contains unexpected trials".into()));
        }
        if rates["procedure"] < rules.minimum_success_rate {
            reasons.push("Transfer success is below policy".into());
        }
        if ["episodes", "summary"]
            .iter()
            .any(|b| rates[*b] - rates["procedure"] > rules.maximum_regression)
        {
            reasons.push("Procedure regresses against a baseline".into());
        }
        if cost > u64::from(rules.maximum_cost_microunits) {
            reasons.push("Evaluation cost exceeds policy".into());
        }
        let mut recognition = BTreeMap::new();
        if let MemoryContent::Procedure {
            contract: Some(contract),
            ..
        } = &input.candidate.record.content
        {
            for criterion in &contract.recognition {
                let observations: Vec<_> = report
                    .trials
                    .iter()
                    .filter(|t| t.arm == QualificationArm::Procedure)
                    .map(|t| {
                        let matches: Vec<_> = t
                            .recognition
                            .iter()
                            .filter(|o| o.criterion == criterion.definition.id)
                            .collect();
                        if matches.len() == 1 {
                            Some(matches[0])
                        } else {
                            None
                        }
                    })
                    .collect();
                recognition.insert(
                    criterion.definition.id.clone(),
                    RecognitionAdoption {
                        question: observations
                            .iter()
                            .all(|o| o.is_some_and(|o| o.packet_valid)),
                        model: observations.iter().all(|o| {
                            o.is_some_and(|o| {
                                o.packet_valid && o.answer_correct && !o.false_negative
                            })
                        }),
                        policy: observations.iter().all(|o| {
                            o.is_some_and(|o| {
                                o.packet_valid
                                    && o.answer_correct
                                    && o.routing_correct
                                    && !o.false_negative
                            })
                        }),
                    },
                );
            }
        }
        let conditions: BTreeSet<_> = suite
            .cases
            .iter()
            .map(|c| c.conditions.join(" AND "))
            .collect();
        let result = AdoptionResult {
            evaluation_job,
            candidate: input.candidate.clone(),
            adopted: reasons.is_empty(),
            reasons,
            rates,
            recognition,
        };
        self.store
            .adopt_procedure(
                auth,
                permit,
                &input,
                report_id,
                conditions.into_iter().collect(),
                result,
            )
            .await
    }
}
fn check_suite(
    suite: &QualificationSuite,
    window: &ConsolidationWindow,
    rules: &ConsolidationPolicy,
) -> Result<()> {
    let groups: BTreeSet<_> = suite.cases.iter().map(|c| c.source_group).collect();
    let ids: BTreeSet<_> = suite.cases.iter().map(|c| &c.id).collect();
    if suite.evaluator_profile != rules.evaluator_profile
        || suite.cases.is_empty()
        || suite.cases.len() > 32
        || ids.len() != suite.cases.len()
        || groups.len() < rules.min_held_out_groups as usize
        || suite
            .cases
            .iter()
            .any(|c| c.id.trim().is_empty() || c.conditions.is_empty())
        || groups
            .iter()
            .any(|id| window.source_groups.iter().any(|g| g.sources.contains(id)))
    {
        return Err(Error::Invalid("Qualification needs independent held-out source groups, conditions and the configured evaluator profile".into()));
    }
    Ok(())
}
