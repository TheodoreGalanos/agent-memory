use crate::Host;
use chrono::Utc;
use memory_domain::{
    contracts::*, coordination::Fence, formation::*, judgement::*, records::*, sources::*,
};
use memory_store::{Error, Result};
use std::collections::BTreeMap;
use uuid::Uuid;

/// What a formation window request yields: authored candidates ready for assessment, or spans of a
/// published task experience that judgement must first turn into candidates.
pub(crate) enum CapturedWindow {
    Candidates(FormationWindow),
    Experience(ExperienceWindow),
}

/// Serialised window bound: packet evidence reads the whole artifact in one bounded read.
const WINDOW_BYTES: usize = 65536;

impl Host {
    pub(crate) async fn capture_window(
        &self,
        auth: &Authority,
        fence: &Fence,
        id: Uuid,
        source: SourceRef,
        operation: String,
        limit: u32,
    ) -> Result<CapturedWindow> {
        if !(1..=32).contains(&limit) || operation.trim().is_empty() || operation.len() > 200 {
            return Err(Error::Invalid(
                "Formation needs an operation and a window of 1 to 32 events".into(),
            ));
        }
        let job = self.store.assigned_job(auth, fence).await?;
        let brief = &job.spec.brief;
        if job.cancel_requested
            || job.deadline <= Utc::now()
            || brief.process != Process::Formation
            || !brief.capabilities.sources.contains(&source)
        {
            return Err(Error::Forbidden);
        }
        let narrow = Authority {
            scope: brief.scope.clone(),
            ..auth.clone()
        };
        let version = self.store.source_version(&narrow, &source).await?;
        if version.kind != SourceKind::ToolEvents {
            return Err(Error::Invalid(
                "Formation expects a versioned tool-event source".into(),
            ));
        }
        let snapshot = version.snapshot_artifact.ok_or(Error::Unavailable)?;
        // Source access is explicitly delegated by the brief, then bounded by its input limit.
        let artifact = self.artifacts.inspect(&narrow, snapshot).await?;
        if artifact.spec.expected_bytes > 1_048_576 {
            return Err(Error::LimitExceeded);
        }
        let mut bytes = Vec::new();
        for start in (0..artifact.spec.expected_bytes).step_by(65536) {
            bytes.extend_from_slice(
                &self
                    .artifacts
                    .read(
                        &narrow,
                        snapshot,
                        start..(start + 65536).min(artifact.spec.expected_bytes),
                    )
                    .await?,
            );
        }
        self.store
            .link_job_artifact(&narrow, fence, snapshot)
            .await?;
        if let Some(prior) = self.store.formation_window(&narrow, fence, id).await? {
            if prior.source != source || prior.operation != operation {
                return Err(Error::Conflict);
            }
            self.publish_work_artifact(
                &narrow,
                fence,
                id,
                "Formation window".into(),
                serde_json::to_string(&prior)?,
                vec![snapshot],
            )
            .await?;
            return Ok(CapturedWindow::Candidates(prior));
        }
        if let Some(prior) = self.store.experience_window(&narrow, fence, id).await? {
            if prior.source != source || prior.operation != operation {
                return Err(Error::Conflict);
            }
            self.publish_work_artifact(
                &narrow,
                fence,
                id,
                "Experience window".into(),
                serde_json::to_string(&prior)?,
                vec![snapshot],
            )
            .await?;
            return Ok(CapturedWindow::Experience(prior));
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes)?;
        if value["schema_version"] != "memory-tool-events/1" {
            return Err(Error::Invalid("Unsupported capture format".into()));
        }
        let events: Vec<ToolEvent> = serde_json::from_value(value["events"].clone())?;
        let after = self
            .store
            .formation_cursor(&narrow, &source, &operation, &brief.policy)
            .await?;
        // Authored candidates carry a FormationInput; anything else is ordinary experience whose
        // candidates judgement has yet to construct.
        let authored = events.get(after as usize).is_none_or(|event| {
            event.kind == "capture"
                && serde_json::from_value::<FormationInput>(event.content.clone()).is_ok()
        });
        if !authored {
            let window = self
                .experience_window(
                    &narrow, fence, id, source, operation, limit, brief, &events, after, snapshot,
                )
                .await?;
            return Ok(CapturedWindow::Experience(window));
        }
        let mut entries: Vec<FormationEntry> = Vec::new();
        for (offset, event) in events
            .iter()
            .enumerate()
            .skip(after as usize)
            .take(limit as usize)
        {
            if event.observed_at > brief.evidence_cutoff {
                break;
            }
            let input: FormationInput = serde_json::from_value(event.content.clone())?;
            input.validate().map_err(Error::Invalid)?;
            if entries
                .first()
                .is_some_and(|first| first.input.episode != input.episode)
            {
                break;
            }
            if input.explicit_contribution
                && (event.origin != Origin::Observed
                    || event.evidential_status != EvidentialStatus::AttributedStatement)
            {
                return Err(Error::Invalid(
                    "Explicit contributions require attributed source statements".into(),
                ));
            }
            if input.explicit_contribution
                && brief
                    .scope
                    .user_id
                    .is_some_and(|id| Some(id) != input.actor_id)
            {
                return Err(Error::Forbidden);
            }
            let position = offset as u32 + 1;
            let locator = self
                .store
                .create_source_locator(
                    &narrow,
                    SourceLocator {
                        id: Uuid::now_v7(),
                        source: source.clone(),
                        locator: Locator::Events {
                            start: position,
                            end: position,
                        },
                    },
                )
                .await?;
            let mut native_locators = Vec::new();
            for id in &input.source_locators {
                let native = self.store.source_locator(&narrow, *id).await?;
                if !brief.capabilities.sources.contains(&native.source) {
                    return Err(Error::Forbidden);
                }
                native_locators.push(native);
            }
            entries.push(FormationEntry {
                position,
                duplicate_records: None,
                previous_deferral: None,
                event: event.clone(),
                input,
                locator,
                native_locators,
            });
        }
        let through = entries.last().map_or(after, |e| e.position);
        let mut window = FormationWindow {
            id,
            job_id: job.id,
            source,
            operation,
            policy: brief.policy.clone(),
            scope: brief.scope.clone(),
            after,
            through,
            cutoff: brief.evidence_cutoff,
            entries,
            remaining: (through as usize) < events.len(),
            examined_without_candidate: vec![],
            experience_window: None,
        };
        for index in 0..window.entries.len() {
            if let Some((records, deferred)) = self
                .store
                .formation_duplicate(&narrow, &window, &window.entries[index].event)
                .await?
            {
                window.entries[index].duplicate_records = Some(records);
                window.entries[index].previous_deferral = deferred;
            }
        }
        let text = serde_json::to_string(&window)?;
        if text.len() > WINDOW_BYTES {
            return Err(Error::LimitExceeded);
        }
        // Save the inspected window first so a crash during artifact publication
        // resumes with the same locators and evidence rather than recapturing it.
        self.store
            .save_formation_window(&narrow, fence, &window)
            .await?;
        self.publish_work_artifact(
            &narrow,
            fence,
            id,
            "Formation window".into(),
            text,
            vec![snapshot],
        )
        .await?;
        Ok(CapturedWindow::Candidates(window))
    }

    /// Groups the next events of a published experience into spans (a tool call with its result, or one
    /// message) until the span limit, the evidence cutoff or the window byte bound. Nothing is a candidate yet.
    #[allow(clippy::too_many_arguments)]
    async fn experience_window(
        &self,
        narrow: &Authority,
        fence: &Fence,
        id: Uuid,
        source: SourceRef,
        operation: String,
        limit: u32,
        brief: &WorkBrief,
        events: &[ToolEvent],
        after: u32,
        snapshot: Uuid,
    ) -> Result<ExperienceWindow> {
        let objective = events
            .iter()
            .find(|e| e.kind == "message" && e.content["role"] == "user")
            .and_then(|e| e.content["text"].as_str())
            .map(|text| text.chars().take(2000).collect::<String>())
            .unwrap_or_else(|| brief.purpose.clone());
        let result_status = events
            .iter()
            .rev()
            .find(|e| e.kind == "result")
            .and_then(|e| e.content["result"]["status"].as_str())
            .map(str::to_owned);
        let mut window = ExperienceWindow {
            id,
            job_id: fence.job_id,
            source,
            operation,
            policy: brief.policy.clone(),
            scope: brief.scope.clone(),
            after,
            through: after,
            cutoff: brief.evidence_cutoff,
            context: ExperienceContext {
                objective,
                result_status,
            },
            spans: vec![],
            remaining: (after as usize) < events.len(),
        };
        let mut offset = after as usize;
        while offset < events.len() && window.spans.len() < limit as usize {
            let event = &events[offset];
            if event.observed_at > brief.evidence_cutoff {
                break;
            }
            let mut span = ExperienceSpan {
                index: window.spans.len() as u32,
                start: offset as u32 + 1,
                end: offset as u32 + 1,
                kind: event.kind.clone(),
                events: vec![event.clone()],
            };
            if event.kind == "tool_call"
                && events.get(offset + 1).is_some_and(|next| {
                    next.kind == "tool_result"
                        && next.content["call_id"] == event.content["call_id"]
                        && next.observed_at <= brief.evidence_cutoff
                })
            {
                span.kind = "exchange".into();
                span.end += 1;
                span.events.push(events[offset + 1].clone());
            }
            let advance = span.events.len();
            window.spans.push(span);
            // Keep the window within one bounded read; a later window continues.
            if !window.spans.is_empty()
                && serde_json::to_string(&window)?.len() > WINDOW_BYTES - 1024
            {
                if window.spans.len() == 1 {
                    return Err(Error::LimitExceeded);
                }
                window.spans.pop();
                break;
            }
            offset += advance;
        }
        window.through = window.spans.last().map_or(after, |s| s.end);
        window.remaining = (window.through as usize) < events.len();
        let text = serde_json::to_string(&window)?;
        self.store
            .save_experience_window(narrow, fence, &window)
            .await?;
        self.publish_work_artifact(
            narrow,
            fence,
            id,
            "Experience window".into(),
            text,
            vec![snapshot],
        )
        .await?;
        Ok(window)
    }

    /// Admits the candidates judgement constructed from an experience window. Each proposal's role
    /// decision must have assessed exactly that span; the candidate adopts an attribution the span
    /// carries and its text is drawn from the span's own material. Spans judged to hold nothing durable
    /// are recorded as examined. Accounted spans must be contiguous from the window start.
    pub(crate) async fn propose_candidates(
        &self,
        auth: &Authority,
        fence: &Fence,
        request: CandidateCommit,
    ) -> Result<FormationWindow> {
        if let Some(prior) = self
            .store
            .formation_window(auth, fence, request.candidate_window_id)
            .await?
        {
            if prior.experience_window != Some(request.window_id) {
                return Err(Error::Conflict);
            }
            return Ok(prior);
        }
        let experience = self
            .store
            .experience_window(auth, fence, request.window_id)
            .await?
            .ok_or(Error::NotFound)?;
        let job = self.store.assigned_job(auth, fence).await?;
        let brief = &job.spec.brief;
        let narrow = Authority {
            scope: brief.scope.clone(),
            ..auth.clone()
        };
        let catalogue: Vec<JudgementDefinition> = serde_json::from_str(include_str!(
            "../../../packages/judgement/src/catalogue-data.json"
        ))?;
        let role_definition = catalogue
            .iter()
            .find(|d| d.id == "J30")
            .ok_or(Error::Unavailable)?;
        let experience_json = serde_json::to_value(&experience)?;
        let mut accounted: BTreeMap<u32, &str> = BTreeMap::new();
        for proposal in &request.proposals {
            if accounted.insert(proposal.span, "proposal").is_some() {
                return Err(Error::Invalid("A span has more than one outcome".into()));
            }
        }
        for examined in &request.examined {
            if accounted.insert(examined.span, "examined").is_some() {
                return Err(Error::Invalid("A span has more than one outcome".into()));
            }
        }
        if accounted
            .keys()
            .enumerate()
            .any(|(expected, span)| *span as usize != expected)
            || accounted.len() > experience.spans.len()
        {
            return Err(Error::Invalid(
                "Accounted spans must be contiguous from the start of the window".into(),
            ));
        }
        let mut entries = Vec::new();
        let mut examined_without_candidate = Vec::new();
        for (span_index, decision_id, expected_role, proposal) in request
            .proposals
            .iter()
            .map(|p| (p.span, p.decision, role_name(&p.input.content), Some(p)))
            .chain(
                request
                    .examined
                    .iter()
                    .map(|e| (e.span, e.decision, "", None)),
            )
        {
            let span = &experience.spans[span_index as usize];
            let decision = self
                .store
                .judgement_decision(auth, fence, decision_id)
                .await?;
            let packet = self.read_judgement_packet(auth, decision.packet_id).await?;
            self.store
                .check_judgement_packet(auth, fence, &packet)
                .await?;
            if packet.local_check_id.is_some()
                || packet.questions.len() != 1
                || packet.evidence_cutoff != experience.cutoff
                || serde_json::to_value(&packet.questions[0].definition)?
                    != serde_json::to_value(role_definition)?
            {
                return Err(Error::Forbidden);
            }
            for (name, pointer) in [
                ("span", format!("/spans/{span_index}")),
                ("context", "/context".to_string()),
            ] {
                let field = packet
                    .evidence
                    .iter()
                    .find(|e| e.name == name)
                    .ok_or(Error::Forbidden)?;
                if field.artifact_id != experience.id
                    || field.pointer != pointer
                    || experience_json.pointer(&pointer) != Some(&field.content)
                {
                    return Err(Error::Forbidden);
                }
            }
            let selected = decision.selected_assessment.ok_or(Error::Forbidden)?;
            let assessment = self.store.assessment(auth, selected).await?;
            let role = choice(&assessment, "J30.role").ok_or(Error::Forbidden)?;
            // A span is examined once its role was judged; a candidate additionally needs the role
            // its content claims. The worker reports why a judged role produced no candidate.
            if assessment.packet_id != packet.id || (proposal.is_some() && role != expected_role) {
                return Err(Error::Forbidden);
            }
            let Some(proposal) = proposal else {
                examined_without_candidate.push(format!("span-{span_index} ({role})"));
                continue;
            };
            let input = &proposal.input;
            input.validate().map_err(Error::Invalid)?;
            if input.explicit_contribution
                || !span
                    .attributions()
                    .contains(&(proposal.origin, proposal.evidential_status))
                || input.objective != experience.context.objective
                || content_texts(&input.content)
                    .into_iter()
                    .any(|text| !span.contains(text))
            {
                return Err(Error::Invalid(
                    "Candidate text must be drawn from the span and adopt its attribution".into(),
                ));
            }
            let locator = self
                .store
                .create_source_locator(
                    &narrow,
                    SourceLocator {
                        id: Uuid::now_v7(),
                        source: experience.source.clone(),
                        locator: Locator::Events {
                            start: span.start,
                            end: span.end,
                        },
                    },
                )
                .await?;
            entries.push(FormationEntry {
                position: span.start,
                duplicate_records: None,
                previous_deferral: None,
                event: ToolEvent {
                    event_id: format!("span-{span_index}"),
                    observed_at: span.events[0].observed_at,
                    kind: "candidate".into(),
                    origin: proposal.origin,
                    evidential_status: proposal.evidential_status,
                    // The input itself is the entry's `input`; the event only names its span.
                    content: serde_json::json!({ "span": span_index }),
                },
                input: input.clone(),
                locator,
                native_locators: vec![],
            });
        }
        let through = accounted
            .keys()
            .last()
            .map_or(experience.after, |i| experience.spans[*i as usize].end);
        let mut window = FormationWindow {
            id: request.candidate_window_id,
            job_id: job.id,
            source: experience.source.clone(),
            operation: experience.operation.clone(),
            policy: experience.policy.clone(),
            scope: experience.scope.clone(),
            after: experience.after,
            through,
            cutoff: experience.cutoff,
            entries,
            remaining: experience.remaining || accounted.len() < experience.spans.len(),
            examined_without_candidate,
            experience_window: Some(experience.id),
        };
        for index in 0..window.entries.len() {
            if let Some((records, deferred)) = self
                .store
                .formation_duplicate(&narrow, &window, &window.entries[index].event)
                .await?
            {
                window.entries[index].duplicate_records = Some(records);
                window.entries[index].previous_deferral = deferred;
            }
        }
        let text = serde_json::to_string(&window)?;
        if text.len() > WINDOW_BYTES {
            return Err(Error::LimitExceeded);
        }
        self.store
            .save_formation_window(&narrow, fence, &window)
            .await?;
        self.publish_work_artifact(
            &narrow,
            fence,
            window.id,
            "Formation window".into(),
            text,
            vec![experience.id],
        )
        .await?;
        Ok(window)
    }
    pub(crate) async fn form_memories(
        &self,
        auth: &Authority,
        fence: &Fence,
        request: FormationCommit,
    ) -> Result<FormationResult> {
        if let Some(result) = self.store.formation_result(auth, fence, &request).await? {
            return Ok(result);
        }
        let window = self
            .store
            .formation_window(auth, fence, request.window_id)
            .await?
            .ok_or(Error::NotFound)?;
        self.store.input_artifact(auth, fence, window.id).await?;
        self.artifacts.read(auth, window.id, 0..0).await?;
        if request.decisions.keys().any(|id| {
            !window
                .entries
                .iter()
                .any(|e| &e.event.event_id == id && !e.suppressed())
        }) {
            return Err(Error::Invalid(
                "Decision does not belong to a selected event".into(),
            ));
        }
        let catalogue: Vec<JudgementDefinition> = serde_json::from_str(include_str!(
            "../../../packages/judgement/src/catalogue-data.json"
        ))?;
        let mut accepted = BTreeMap::new();
        let mut result = FormationResult {
            window_id: window.id,
            cursor: window.through,
            records: vec![],
            duplicate_events: BTreeMap::new(),
            deferred: vec![],
            coverage: Coverage {
                examined: vec![],
                unexamined: vec![],
            },
            unresolved: vec![],
            inspected: vec![],
            common_source_groups: vec![SupportGroup {
                source: window.source.clone(),
                event_ids: window
                    .entries
                    .iter()
                    .map(|e| e.event.event_id.clone())
                    .collect(),
            }],
            judgement_usage: BTreeMap::new(),
            usage: Usage {
                status: UsageStatus::Unknown,
                input_tokens: None,
                output_tokens: None,
                cost: None,
            },
        };
        result.coverage.examined.extend(
            window
                .examined_without_candidate
                .iter()
                .map(|span| format!("{span}: examined, no candidate")),
        );
        for (index, entry) in window.entries.iter().enumerate() {
            result.inspected.push(entry.locator.clone());
            for locator in &entry.native_locators {
                result.coverage.unexamined.push(format!(
                    "Native locator {} retained; content not inspected by formation",
                    locator.id
                ));
                if !result
                    .common_source_groups
                    .iter()
                    .any(|g| g.source == locator.source)
                {
                    result.common_source_groups.push(SupportGroup {
                        source: locator.source.clone(),
                        event_ids: vec![],
                    });
                }
                let group = result
                    .common_source_groups
                    .iter_mut()
                    .find(|g| g.source == locator.source)
                    .expect("inserted group");
                if !group.event_ids.contains(&entry.event.event_id) {
                    group.event_ids.push(entry.event.event_id.clone());
                }
            }
            result.coverage.examined.push(entry.event.event_id.clone());
            result
                .coverage
                .examined
                .extend(entry.input.coverage.examined.clone());
            result
                .coverage
                .unexamined
                .extend(entry.input.coverage.unexamined.clone());
            result.unresolved.extend(entry.input.uncertainty.clone());
            if entry.duplicate_records.is_some() {
                if let Some(deferred) = &entry.previous_deferral {
                    if deferred.required {
                        result.unresolved.push(format!(
                            "Required contribution {} remains deferred: {}",
                            deferred.event_id, deferred.reason
                        ));
                    }
                    result.deferred.push(deferred.clone());
                }
                continue;
            }
            let reason = if entry.suppressed() {
                Some("Temporary or hypothetical material was not explicitly selected".into())
            } else if let Some(id) = request.decisions.get(&entry.event.event_id) {
                let decision = self.store.judgement_decision(auth, fence, *id).await?;
                let packet = self.read_judgement_packet(auth, decision.packet_id).await?;
                self.store
                    .check_judgement_packet(auth, fence, &packet)
                    .await?;
                let expected = entry.families();
                if packet.local_check_id.is_some()
                    || packet.questions.len() != expected.len()
                    || packet.evidence_cutoff != window.cutoff
                {
                    return Err(Error::Forbidden);
                }
                for family in &expected {
                    let question = packet
                        .questions
                        .iter()
                        .find(|q| q.definition.id == *family)
                        .ok_or(Error::Forbidden)?;
                    let definition = catalogue
                        .iter()
                        .find(|d| d.id == *family)
                        .ok_or(Error::Forbidden)?;
                    if serde_json::to_value(&question.definition)?
                        != serde_json::to_value(definition)?
                    {
                        return Err(Error::Forbidden);
                    }
                    for name in &definition.input_requirements {
                        let field = packet
                            .evidence
                            .iter()
                            .find(|e| &e.name == name)
                            .ok_or(Error::Forbidden)?;
                        let pointer = FormationEntry::evidence_pointer(index, name)
                            .ok_or(Error::Forbidden)?;
                        if field.artifact_id != window.id
                            || field.pointer != pointer
                            || serde_json::to_value(&window)?.pointer(&pointer)
                                != Some(&field.content)
                        {
                            return Err(Error::Forbidden);
                        }
                    }
                }
                for id in &decision.assessment_ids {
                    result
                        .judgement_usage
                        .insert(*id, self.store.assessment(auth, *id).await?.usage);
                }
                if let Some(id) = decision.selected_assessment {
                    let assessment = self.store.assessment(auth, id).await?;
                    if assessment.packet_id != packet.id {
                        return Err(Error::Forbidden);
                    }
                    let reason = disposition(entry, &assessment);
                    if reason.is_none() {
                        accepted.insert(
                            entry.event.event_id.clone(),
                            PolicyDecision {
                                policy: window.policy.clone(),
                                action: PolicyAction::Retain,
                                reason: format!(
                                    "Formation support and status checks passed; decision {}",
                                    decision.id
                                ),
                                constraints: vec![
                                    "Candidate only; no promotion or intention execution".into(),
                                ],
                                required_evidence: vec![],
                                expires_at: None,
                            },
                        );
                    }
                    reason
                } else {
                    Some("No usable selected assessment".into())
                }
            } else {
                Some("Semantic support checks are missing".into())
            };
            if let Some(reason) = reason {
                let required = entry.input.retention == Retention::Required;
                if required {
                    result.unresolved.push(format!(
                        "Required contribution {} deferred: {reason}",
                        entry.event.event_id
                    ));
                }
                result.deferred.push(DeferredContribution {
                    event_id: entry.event.event_id.clone(),
                    reason,
                    required,
                });
            }
        }
        if window.remaining {
            result
                .coverage
                .unexamined
                .push("Events after this bounded window or evidence cutoff".into());
        }
        result.usage.input_tokens = result
            .judgement_usage
            .values()
            .try_fold(0u32, |sum, usage| sum.checked_add(usage.input_tokens?));
        result.usage.output_tokens = result
            .judgement_usage
            .values()
            .try_fold(0u32, |sum, usage| sum.checked_add(usage.output_tokens?));
        result.usage.status = UsageStatus::Partial; // Monetary telemetry is retained in the assessment/budget receipts, not guessed.
        self.store
            .commit_formation(auth, fence, &window, &request, &accepted, result)
            .await
    }
}
fn role_name(content: &MemoryContent) -> &'static str {
    match content {
        MemoryContent::Episode { .. } => "episode",
        MemoryContent::Knowledge { .. } => "knowledge",
        MemoryContent::Procedure { .. } => "procedure",
        MemoryContent::Intention { .. } => "intention",
    }
}
/// The free text a constructed candidate asserts; every item must come from the span itself.
fn content_texts(content: &MemoryContent) -> Vec<&str> {
    match content {
        MemoryContent::Episode {
            observations,
            actions,
            corrections,
            outcome,
            ..
        } => observations
            .iter()
            .chain(actions)
            .chain(corrections)
            .map(String::as_str)
            .chain(std::iter::once(outcome.as_str()))
            .collect(),
        MemoryContent::Knowledge { statement, .. } => vec![statement.as_str()],
        MemoryContent::Procedure {
            purpose, method, ..
        } => {
            let mut texts = vec![purpose.as_str()];
            if let ProcedureForm::Advisory { steps, .. } = method {
                texts.extend(steps.iter().map(String::as_str));
            }
            texts
        }
        MemoryContent::Intention {
            purpose, trigger, ..
        } => vec![purpose.as_str(), trigger.as_str()],
    }
}
fn choice<'a>(assessment: &'a SemanticAssessment, key: &str) -> Option<&'a str> {
    match assessment.answers.get(key) {
        Some(JudgementAnswer::Choice { choice, .. }) => Some(choice),
        _ => None,
    }
}
fn disposition(entry: &FormationEntry, assessment: &SemanticAssessment) -> Option<String> {
    let status = serde_json::to_value(entry.event.evidential_status).ok()?;
    let valid = choice(assessment, "J01.assessment") == status.as_str()
        && choice(assessment, "J01.faithfulness") == Some("yes")
        && choice(assessment, "J02.assessment") == Some("supports")
        && (entry.input.boundary_explicit
            || matches!(
                choice(assessment, "J03.assessment"),
                Some("boundary" | "correction" | "continuation")
            ))
        && (!matches!(entry.input.content, MemoryContent::Procedure { .. })
            || matches!(
                choice(assessment, "J04.assessment"),
                Some("local_convention" | "conditional_method" | "untested_generalisation")
            ))
        && (!matches!(entry.input.content, MemoryContent::Intention { .. })
            || choice(assessment, "J05.assessment") == Some("commitment"))
        && (!entry.input.explicit_contribution
            || matches!(
                choice(assessment, "J27.assessment"),
                Some("task_local" | "persistent_preference" | "proposed_shared_rule")
            ));
    (!valid).then(|| "Support, status or family-specific interpretation remains unresolved".into())
}
