use crate::Host;
use chrono::Utc;
use memory_domain::{
    contracts::*, coordination::Fence, formation::*, judgement::*, records::*, sources::*,
};
use memory_store::{Error, Result};
use std::collections::BTreeMap;
use uuid::Uuid;

impl Host {
    pub(crate) async fn capture_window(
        &self,
        auth: &Authority,
        fence: &Fence,
        id: Uuid,
        source: SourceRef,
        operation: String,
        limit: u32,
    ) -> Result<FormationWindow> {
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
            return Ok(prior);
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
        if text.len() > 65536 {
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
