use crate::Host;
use chrono::Utc;
use memory_domain::{activation::*, contracts::*, coordination::Fence, judgement::*, records::*};
use memory_store::{Error, Result};
use std::collections::BTreeMap;
use uuid::Uuid;

impl Host {
    pub(crate) async fn activate(
        &self,
        auth: &Authority,
        fence: &Fence,
        id: Uuid,
        query: ActivationQuery,
        cursor: Option<ActivationCursor>,
    ) -> Result<ActivationWindow> {
        let job = self.store.assigned_job(auth, fence).await?;
        if job.cancel_requested
            || job.deadline <= Utc::now()
            || job.spec.brief.process != Process::Activation
            || !job.spec.brief.scope.permits(&query.scope)
        {
            return Err(Error::Forbidden);
        }
        let narrow = Authority {
            scope: query.scope.clone(),
            ..auth.clone()
        };
        let mut window = if let Some(prior) = self.store.activation_window(auth, fence, id).await? {
            if serde_json::to_value(&prior.query)? != serde_json::to_value(&query)?
                || serde_json::to_value(&prior.cursor)? != serde_json::to_value(&cursor)?
            {
                return Err(Error::Conflict);
            }
            // Never return a retained window after a candidate loses access/current availability.
            self.check_activation_access(&narrow, &prior).await?;
            prior
        } else {
            let mut window = self.store.activation(&narrow, id, query, cursor).await?;
            for c in &mut window.candidates {
                if let MemoryContent::Procedure {
                    method: ProcedureForm::Executable { artifact_id, .. },
                    ..
                } = c.memory.record.content
                {
                    let artifact = self.artifacts.inspect(&narrow, artifact_id).await?;
                    if artifact.spec.expected_bytes <= 16384 {
                        let bytes = self
                            .artifacts
                            .read(&narrow, artifact_id, 0..artifact.spec.expected_bytes)
                            .await?;
                        c.definition = String::from_utf8(bytes.to_vec()).ok();
                    }
                    if c.definition.is_none() {
                        window.coverage.push("Executable definition exceeds the text inspection bound; further inspection is required".into());
                    }
                }
            }
            window.job_id = Some(job.id);
            if serde_json::to_vec(&window)?.len() > 60000 {
                return Err(Error::LimitExceeded);
            }
            self.store
                .save_activation_window(auth, fence, &window)
                .await?;
            window
        };
        window.job_id = Some(job.id);
        self.publish_work_artifact(
            auth,
            fence,
            id,
            "Activation candidates".into(),
            serde_json::to_string(&window)?,
            vec![],
        )
        .await?;
        Ok(window)
    }

    async fn check_activation_access(
        &self,
        auth: &Authority,
        window: &ActivationWindow,
    ) -> Result<()> {
        for reference in window
            .eligible
            .iter()
            .chain(window.candidates.iter().map(|c| &c.memory.reference))
        {
            self.store
                .activation_memory(auth, reference, &window.query, window.cutoff)
                .await?;
        }
        for c in &window.candidates {
            if let MemoryContent::Procedure {
                method: ProcedureForm::Executable { artifact_id, .. },
                ..
            } = c.memory.record.content
            {
                if !matches!(
                    self.artifacts.inspect(auth, artifact_id).await?.state,
                    memory_domain::sources::ArtifactState::Ready
                ) {
                    return Err(Error::Unavailable);
                }
            }
        }
        Ok(())
    }

    async fn activation_answers(
        &self,
        auth: &Authority,
        fence: &Fence,
        window: &ActivationWindow,
        id: Option<&Uuid>,
        family: &str,
        fields: BTreeMap<String, String>,
    ) -> Result<BTreeMap<String, JudgementAnswer>> {
        let Some(id) = id else {
            return Ok(BTreeMap::new());
        };
        let decision = self.store.judgement_decision(auth, fence, *id).await?;
        let packet = self.read_judgement_packet(auth, decision.packet_id).await?;
        self.store
            .check_judgement_packet(auth, fence, &packet)
            .await?;
        let catalogue: Vec<JudgementDefinition> = serde_json::from_str(include_str!(
            "../../../packages/judgement/src/catalogue-data.json"
        ))?;
        let canonical = catalogue
            .iter()
            .find(|d| d.id == family)
            .ok_or(Error::Forbidden)?;
        if packet.local_check_id.is_some()
            || packet.questions.len() != 1
            || serde_json::to_value(&packet.questions[0].definition)?
                != serde_json::to_value(canonical)?
            || packet.evidence.len() != fields.len()
        {
            return Err(Error::Forbidden);
        }
        let data = serde_json::to_value(window)?;
        for (name, pointer) in fields {
            let evidence = packet
                .evidence
                .iter()
                .find(|e| e.name == name)
                .ok_or(Error::Forbidden)?;
            if evidence.artifact_id != window.id
                || evidence.pointer != pointer
                || data.pointer(&pointer) != Some(&evidence.content)
            {
                return Err(Error::Forbidden);
            }
        }
        let Some(selected) = decision.selected_assessment else {
            return Ok(BTreeMap::new());
        };
        let assessment = self.store.assessment(auth, selected).await?;
        assessment.validate(&packet).map_err(Error::Invalid)?;
        Ok(assessment.answers)
    }

    pub(crate) async fn select_activation(
        &self,
        auth: &Authority,
        fence: &Fence,
        selection: ActivationSelection,
    ) -> Result<ContextPackage> {
        let job = self.store.assigned_job(auth, fence).await?;
        if job.cancel_requested
            || job.deadline <= Utc::now()
            || job.spec.brief.process != Process::Activation
        {
            return Err(Error::Forbidden);
        }
        let window = self
            .store
            .activation_window(auth, fence, selection.window_id)
            .await?
            .ok_or(Error::NotFound)?;
        if !job.spec.brief.scope.permits(&window.query.scope) {
            return Err(Error::Forbidden);
        }
        let narrow = Authority {
            scope: window.query.scope.clone(),
            ..auth.clone()
        };
        // Recheck every exposed record before returning this package, even a deferred one.
        self.check_activation_access(&narrow, &window).await?;
        let mut dispositions = vec![];
        for (i, c) in window.candidates.iter().enumerate() {
            let candidate = format!("/candidates/{i}");
            let role = self
                .activation_answers(
                    auth,
                    fence,
                    &window,
                    selection.decisions.get(&format!("{i}/J06")),
                    "J06",
                    BTreeMap::from([
                        ("candidate".into(), candidate.clone()),
                        ("current_question".into(), "/query/question".into()),
                        ("task_context".into(), "/query/task_context".into()),
                    ]),
                )
                .await?;
            let roles: Vec<String> = ["direct", "background", "exception", "contradiction"]
                .into_iter()
                .filter(|r| choice(&role, &format!("J06.{r}")) == Some("yes"))
                .map(str::to_owned)
                .collect();
            let mut disposition = ActivationDisposition {
                memory: c.memory.reference.clone(),
                roles,
                applicability: if role.is_empty() {
                    "unresolved"
                } else {
                    "evidence"
                }
                .into(),
                missing_conditions: vec![],
            };
            if let MemoryContent::Procedure { .. } = c.memory.record.content {
                let relevance = self
                    .activation_answers(
                        auth,
                        fence,
                        &window,
                        selection.decisions.get(&format!("{i}/J07")),
                        "J07",
                        BTreeMap::from([
                            ("method".into(), candidate.clone()),
                            ("question".into(), "/query/question".into()),
                            ("task_context".into(), "/query/task_context".into()),
                        ]),
                    )
                    .await?;
                disposition.applicability = match choice(&relevance, "J07.assessment") {
                    Some("relevant") => "applicable",
                    Some("irrelevant") => "irrelevant",
                    _ => "unresolved",
                }
                .into();
                for (j, condition) in c.conditions.iter().enumerate() {
                    let result = self
                        .activation_answers(
                            auth,
                            fence,
                            &window,
                            selection.decisions.get(&format!("{i}/J08/{j}")),
                            "J08",
                            BTreeMap::from([
                                ("method".into(), candidate.clone()),
                                (
                                    "prerequisites".into(),
                                    format!("/candidates/{i}/conditions/{j}"),
                                ),
                                ("evidence".into(), "/query/task_context".into()),
                            ]),
                        )
                        .await?;
                    if choice(&result, "J08.assessment") == Some("unmet") {
                        disposition.applicability = "inapplicable".into();
                    }
                    if choice(&result, "J08.assessment") != Some("established") {
                        disposition.missing_conditions.push(condition.clone());
                    }
                }
                if !disposition.missing_conditions.is_empty()
                    && disposition.applicability == "applicable"
                {
                    disposition.applicability = "needs_investigation".into();
                }
                if matches!(
                    c.memory.record.content,
                    MemoryContent::Procedure {
                        method: ProcedureForm::Executable { .. },
                        ..
                    }
                ) && c.definition.is_none()
                {
                    disposition
                        .missing_conditions
                        .push("Inspect full executable definition".into());
                    if disposition.applicability == "applicable" {
                        disposition.applicability = "needs_investigation".into();
                    }
                }
                if matches!(
                    c.memory.record.qualification,
                    Qualification::Withdrawn { .. }
                ) {
                    disposition.applicability = "withdrawn".into();
                }
                if !matches!(
                    c.memory.record.qualification,
                    Qualification::Evaluated { .. }
                ) && disposition.applicability == "applicable"
                {
                    disposition.applicability = "candidate_method".into();
                }
            }
            dispositions.push(disposition);
        }
        let mut selected = vec![];
        let mut deferred = vec![];
        let mut bytes = 0;
        for (index, g) in window.groups.iter().enumerate() {
            let relevant = g.members.iter().any(|r| {
                dispositions.iter().any(|d| {
                    same(r, &d.memory)
                        && (!d.roles.is_empty()
                            || ["applicable", "candidate_method", "needs_investigation"]
                                .contains(&d.applicability.as_str()))
                })
            });
            if !g.complete || !relevant {
                deferred.push(format!(
                    "Group {index}: {}",
                    if !g.complete {
                        "incomplete related evidence"
                    } else {
                        "no established relevance"
                    }
                ));
                continue;
            }
            let additions: Vec<_> = g
                .members
                .iter()
                .filter(|r| !window.query.existing.iter().any(|e| same(e, r)))
                .collect();
            let size: usize = additions
                .iter()
                .map(|r| {
                    window
                        .candidates
                        .iter()
                        .find(|c| same(r, &c.memory.reference))
                        .expect("group candidate")
                })
                .map(serde_json::to_vec)
                .collect::<std::result::Result<Vec<_>, _>>()?
                .iter()
                .map(Vec::len)
                .sum();
            if bytes + size > window.query.context_bytes as usize {
                deferred.push(format!(
                    "Group {index}: complete group exceeds context allowance"
                ));
                continue;
            }
            bytes += size;
            selected.extend(additions.into_iter().cloned());
        }
        let reason = if selected.is_empty() {
            if window.candidates.is_empty() {
                "No candidates matched within the inspected coverage; no addition"
            } else if deferred.is_empty() {
                "Existing context is sufficient for the retrieved groups; no addition"
            } else {
                "No safe addition selected; inspect deferred work and coverage"
            }
        } else {
            "Relevant evidence groups selected; renderer controls final admission"
        }
        .into();
        Ok(ContextPackage {
            window,
            selected,
            dispositions,
            deferred,
            reason,
        })
    }
}
fn choice<'a>(answers: &'a BTreeMap<String, JudgementAnswer>, key: &str) -> Option<&'a str> {
    match answers.get(key) {
        Some(JudgementAnswer::Choice { choice, .. }) => Some(choice),
        _ => None,
    }
}
fn same(a: &MemoryRef, b: &MemoryRef) -> bool {
    a.memory_id == b.memory_id && a.revision == b.revision
}
