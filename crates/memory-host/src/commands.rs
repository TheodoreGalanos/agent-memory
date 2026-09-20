//! Dispatch authenticated commands to their owning service.
use super::{Host, formation, judgements};
use memory_domain::{
    contracts::Authority,
    coordination::{HostRequest, HostResponse},
};
use memory_store::Error;

impl Host {
    pub(super) async fn execute(
        &self,
        auth: &Authority,
        request: HostRequest,
    ) -> memory_store::Result<HostResponse> {
        use HostRequest as R;
        use HostResponse as O;
        Ok(match request {
            R::User { request } => O::User {
                result: Box::new(self.store.user_request(auth, *request).await?),
            },
            R::RecordContext { fence, manifest } => {
                self.store.record_context(auth, &fence, *manifest).await?;
                O::Done { affected: 1 }
            }
            R::CheckProvider { fence, provider } => {
                self.store
                    .check_execution(auth, &fence, Some(&provider), None)
                    .await?;
                O::Done { affected: 1 }
            }
            R::ListDeletions { after_id, limit } => O::Deletions {
                reports: self.store.deletions(auth, after_id, limit).await?,
            },
            R::ReconcileDeletedEffect {
                deletion,
                effect_id,
                resolution,
                evidence_artifact,
            } => {
                self.store
                    .reconcile_deleted_effect(
                        auth,
                        deletion,
                        effect_id,
                        resolution,
                        evidence_artifact,
                    )
                    .await?;
                O::Deletion {
                    report: self.store.deletion_report(auth, deletion).await?,
                }
            }
            R::ContinueDeletedWork {
                deletion,
                old_job,
                command,
            } => O::Job {
                job: Box::new(
                    self.store
                        .continue_deleted_work(auth, deletion, old_job, *command)
                        .await?,
                ),
            },
            R::BeginDeletion { request } => O::Deletion {
                report: self.store.begin_deletion(auth, request).await?,
            },
            R::InspectDeletion { id } => O::Deletion {
                report: self.store.deletion_report(auth, id).await?,
            },
            R::PurgeDeletion { id } => O::Deletion {
                report: self.artifacts.purge_deletion(auth, id).await?,
            },
            R::AcknowledgePurge {
                id,
                boundary,
                container,
            } => O::Deletion {
                report: self
                    .store
                    .acknowledge_purge(auth, id, boundary, &container)
                    .await?,
            },
            R::RestoreDeletionRegistry { reports } => {
                self.store.restore_deletion_registry(auth, reports).await?;
                O::Done { affected: 0 }
            }
            R::AdmitJudgement { fence, packet } => O::JudgementPacket {
                packet: Box::new(self.admit_judgement(auth, &fence, *packet).await?),
            },
            R::CheckJudgement {
                fence,
                packet_id,
                provider,
            } => O::JudgementPacket {
                packet: Box::new(
                    self.check_judgement(auth, &fence, packet_id, &provider)
                        .await?,
                ),
            },
            R::RecordAssessment { fence, assessment } => {
                let packet = self
                    .check_judgement(auth, &fence, assessment.packet_id, &assessment.provider)
                    .await?;
                self.store
                    .input_artifact(auth, &fence, assessment.raw_artifact_id)
                    .await?;
                let raw = self
                    .artifacts
                    .inspect(auth, assessment.raw_artifact_id)
                    .await?;
                if !raw.spec.dependencies.contains(&packet.id) {
                    return Err(Error::Forbidden);
                }
                O::Assessment {
                    assessment: Some(Box::new(
                        self.store
                            .record_assessment(auth, &fence, &packet, *assessment)
                            .await?,
                    )),
                }
            }
            R::ReuseAssessment {
                fence,
                assessment_id,
                packet_id,
                provider,
                model_release,
            } => O::Assessment {
                assessment: self
                    .reuse_assessment(
                        auth,
                        &fence,
                        assessment_id,
                        packet_id,
                        &provider,
                        &model_release,
                    )
                    .await?,
            },
            R::RecordJudgementDecision { fence, decision } => {
                self.store
                    .input_artifact(auth, &fence, decision.packet_id)
                    .await?;
                let packet = self.read_judgement_packet(auth, decision.packet_id).await?;
                for id in &decision.assessment_ids {
                    let assessment = self.store.assessment(auth, *id).await?;
                    let basis = self
                        .read_judgement_packet(auth, assessment.packet_id)
                        .await?;
                    if !judgements::equivalent(&basis, &packet)? {
                        return Err(Error::Forbidden);
                    }
                }
                O::JudgementDecision {
                    decision: Box::new(
                        self.store
                            .record_judgement_decision(auth, &fence, &packet, *decision)
                            .await?,
                    ),
                }
            }
            R::RegisterTaskCheck { fence, check } => O::TaskCheck {
                check: Box::new(self.store.register_task_check(auth, &fence, *check).await?),
            },
            R::CheckDecision { fence, choice, .. } => {
                let (accepted, reasons) = self.check_decision(auth, &fence, choice).await?;
                O::DecisionCheck { accepted, reasons }
            }
            R::InspectTaskCheck { fence, id } => O::TaskCheck {
                check: Box::new(self.store.task_local_check(auth, &fence, id).await?),
            },
            R::RetireTaskCheck { fence, id } => {
                self.store.retire_task_check(auth, &fence, id).await?;
                O::Done { affected: 1 }
            }
            R::CreateBudget { budget } => O::Budget {
                budget: Box::new(self.store.create_budget(auth, *budget).await?),
            },
            R::Submit { command } => O::Job {
                job: Box::new(self.store.submit_job(auth, *command).await?),
            },
            R::SpawnChild {
                fence,
                request_id,
                brief,
                deadline,
                reuse_job_id,
            } => O::Job {
                job: Box::new(
                    self.store
                        .spawn_child(auth, &fence, request_id, *brief, deadline, reuse_job_id)
                        .await?,
                ),
            },
            R::ChildJobs { fence, ids } => O::Children {
                jobs: self.store.child_jobs(auth, &fence, &ids).await?,
            },
            R::WaitChildren {
                fence,
                ids,
                ready_at,
            } => {
                self.store
                    .wait_children(auth, &fence, &ids, ready_at)
                    .await?;
                O::Done { affected: 1 }
            }
            R::ReadInput {
                fence,
                artifact_id,
                offset,
                limit,
            } => {
                if limit == 0 || limit > 65536 {
                    return Err(Error::LimitExceeded);
                }
                self.store.input_artifact(auth, &fence, artifact_id).await?;
                let artifact = self.artifacts.inspect(auth, artifact_id).await?;
                let end = offset
                    .checked_add(limit)
                    .ok_or(Error::LimitExceeded)?
                    .min(artifact.spec.expected_bytes);
                let data = self.artifacts.read(auth, artifact_id, offset..end).await?;
                O::ArtifactData {
                    text: String::from_utf8(data.to_vec())
                        .map_err(|_| Error::Invalid("Text input must be UTF-8".into()))?,
                }
            }
            R::PublishArtifact {
                fence,
                request_id,
                label,
                text,
                dependencies,
            } => O::Artifact {
                artifact: Box::new(
                    self.publish_work_artifact(auth, &fence, request_id, label, text, dependencies)
                        .await?,
                ),
            },
            R::ConsolidationWindow {
                fence,
                id,
                selection,
            } => O::ConsolidationWindow {
                window: Box::new(
                    self.consolidation_window(auth, &fence, id, selection)
                        .await?,
                ),
            },
            R::ReviewConsolidation {
                fence,
                id,
                proposal,
            } => O::ConsolidationReview {
                review: Box::new(
                    self.review_consolidation(auth, &fence, id, *proposal)
                        .await?,
                ),
            },
            R::CommitConsolidation { fence, request } => O::ConsolidationResult {
                result: Box::new(self.commit_consolidation(auth, &fence, request).await?),
            },
            R::StartQualification {
                fence,
                request_id,
                review_id,
                suite_id,
            } => O::Job {
                job: Box::new(
                    self.start_qualification(auth, &fence, request_id, review_id, suite_id)
                        .await?,
                ),
            },
            R::AdoptProcedure {
                fence,
                evaluation_job,
            } => O::AdoptionResult {
                result: Box::new(self.adopt_procedure(auth, &fence, evaluation_job).await?),
            },
            R::EmbeddingInputs { after, limit } => O::EmbeddingInputs {
                inputs: self.store.embedding_inputs(auth, after, limit).await?,
            },
            R::Activate {
                fence,
                id,
                query,
                cursor,
            } => O::ActivationWindow {
                window: Box::new(self.activate(auth, &fence, id, *query, cursor).await?),
            },
            R::SelectActivation { fence, selection } => O::ContextPackage {
                package: Box::new(self.select_activation(auth, &fence, selection).await?),
            },
            R::SaveEmbedding {
                reference,
                embedding,
            } => {
                self.store
                    .save_embedding(auth, &reference, embedding)
                    .await?;
                O::Done { affected: 1 }
            }
            R::FormationWindow {
                fence,
                id,
                source,
                operation,
                limit,
            } => match self
                .capture_window(auth, &fence, id, source, operation, limit)
                .await?
            {
                formation::CapturedWindow::Candidates(window) => O::FormationWindow {
                    window: Box::new(window),
                },
                formation::CapturedWindow::Experience(window) => O::ExperienceWindow {
                    window: Box::new(window),
                },
            },
            R::ProposeCandidates { fence, request } => O::FormationWindow {
                window: Box::new(self.propose_candidates(auth, &fence, request).await?),
            },
            R::CommitFormation { fence, request } => O::FormationResult {
                result: Box::new(self.form_memories(auth, &fence, *request).await?),
            },
            R::InspectJob { job_id } => O::Job {
                job: Box::new(self.store.job(auth, job_id).await?),
            },
            R::Backup { directory } => O::Backup {
                manifest: Box::new(self.backup(auth, directory).await?),
            },
            R::ClaimNext {
                processes,
                lease_seconds,
            } => self.claim_next(auth, &processes, lease_seconds).await?,
            R::IssueWorkerCredential { job_id } => O::WorkerCredential {
                credential: Box::new(self.issue_worker_credential(auth, job_id).await?),
            },
            R::IngestSource { source, content } => O::Source {
                source: Box::new(self.ingest_source(auth, source, content).await?),
            },
            R::PublishExperience { fence, events } => O::Source {
                source: Box::new(self.publish_experience(auth, &fence, events).await?),
            },
            R::Experience { job_id } => {
                let reference = self
                    .store
                    .experience(auth, job_id)
                    .await?
                    .ok_or(Error::NotFound)?;
                O::Source {
                    source: Box::new(self.store.source_version(auth, &reference).await?),
                }
            }
            R::ScheduledWork {
                trigger_kind,
                trigger_id,
            } => O::ScheduledWork {
                mark: self
                    .store
                    .scheduled_work(auth, &trigger_kind, trigger_id)
                    .await?
                    .map(Box::new),
            },
            R::Claim {
                job_id,
                lease_seconds,
            } => O::Assignment {
                assignment: Box::new(self.store.claim_job(auth, job_id, lease_seconds).await?),
            },
            R::InspectAssignment { fence } => O::Assignment {
                assignment: Box::new(self.store.inspect_assignment(auth, &fence).await?),
            },
            R::Start { fence, composition } => O::Job {
                job: Box::new(
                    self.store
                        .start_job(auth, &fence, composition.as_deref())
                        .await?,
                ),
            },
            R::Renew {
                fence,
                lease_seconds,
            } => O::Assignment {
                assignment: Box::new(
                    self.store
                        .renew_assignment(auth, &fence, lease_seconds)
                        .await?,
                ),
            },
            R::Wait {
                fence,
                reason,
                ready_at,
            } => {
                self.store.wait_job(auth, &fence, &reason, ready_at).await?;
                O::Done { affected: 1 }
            }
            R::Cancel { job_id } => {
                self.store.cancel_job(auth, job_id).await?;
                O::Done { affected: 1 }
            }
            R::AcknowledgeCancellation { fence } => O::Job {
                job: Box::new(self.store.acknowledge_cancellation(auth, &fence).await?),
            },
            R::RecordValidation {
                fence,
                verdict,
                reasons,
            } => O::Job {
                job: Box::new(
                    self.store
                        .record_validation(auth, &fence, verdict, reasons)
                        .await?,
                ),
            },
            R::Complete {
                request_id,
                fence,
                result,
            } => O::Job {
                job: Box::new(
                    self.publish_result(auth, request_id, &fence, *result)
                        .await?,
                ),
            },
            R::CommitMemories { command } => O::Memories {
                memories: self.store.commit_memories(auth, *command).await?,
            },
            R::Reserve {
                fence,
                provider_attempt,
                maximum,
                final_result,
            } => O::Reservation {
                reservation: self
                    .store
                    .reserve_usage(auth, &fence, &provider_attempt, maximum, final_result)
                    .await?,
            },
            R::SettleUsage {
                fence,
                reservation_id,
                observed,
            } => O::Reservation {
                reservation: self
                    .store
                    .settle_usage(auth, &fence, reservation_id, observed)
                    .await?,
            },
            R::BudgetUsage { budget_id } => O::BudgetUsage {
                usage: Box::new(self.store.budget_usage(auth, budget_id).await?),
            },
            R::PrepareEffect { fence, request } => O::Effect {
                effect: self.store.prepare_effect(auth, &fence, request).await?,
            },
            R::BeginEffect { fence, effect_id } => O::Effect {
                effect: self.store.begin_effect(auth, &fence, effect_id).await?,
            },
            R::ReportEffect {
                fence,
                effect_id,
                state,
                receipt,
            } => O::Effect {
                effect: self
                    .store
                    .report_effect(auth, &fence, effect_id, state, receipt)
                    .await?,
            },
            R::InspectEffect { effect_id } => O::Effect {
                effect: self.store.effect(auth, effect_id).await?,
            },
            R::ReconcileEffect {
                effect_id,
                resolution,
                evidence,
            } => O::Effect {
                effect: self
                    .store
                    .reconcile_effect(auth, effect_id, resolution, evidence)
                    .await?,
            },
            R::Events { after, limit } => O::Events {
                page: self.store.events(auth, after, limit).await?,
            },
            R::ConsumeEvent {
                consumer,
                event_id,
                command,
            } => O::Job {
                job: Box::new(
                    self.store
                        .consume_event(auth, &consumer, event_id, *command)
                        .await?,
                ),
            },
            R::AllocateArtifact { id, spec } => O::Artifact {
                artifact: Box::new(self.artifacts.allocate_named(auth, id, *spec).await?),
            },
            R::RecoverArtifactUpload { id, revision } => O::Artifact {
                artifact: Box::new(self.artifacts.recover_upload(auth, id, revision).await?),
            },
            R::ReviewMaintenance { fence, id, request } => O::MaintenanceReview {
                review: Box::new(self.review_maintenance(auth, &fence, id, *request).await?),
            },
            R::CommitMaintenance { fence, request } => O::MaintenanceResult {
                result: Box::new(self.commit_maintenance(auth, &fence, request).await?),
            },
            R::MemoryChanges {
                fence,
                after,
                limit,
            } => O::Changes {
                page: self
                    .store
                    .memory_changes(&self.maintenance_scope(auth, &fence).await?, after, limit)
                    .await?,
            },
            R::CurrentMemories { fence, references } => O::Memories {
                memories: self.current_memories(auth, &fence, references).await?,
            },
            R::IntentionCheck {
                fence,
                id,
                occurrence_id,
                kind,
            } => O::IntentionCheck {
                check: Box::new(
                    self.intention_check(auth, &fence, id, occurrence_id, kind)
                        .await?,
                ),
            },
            R::ApplyIntentionCheck {
                fence,
                check_id,
                decisions,
            } => O::Intention {
                occurrence: Box::new(
                    self.apply_intention_check(auth, &fence, check_id, decisions)
                        .await?,
                ),
            },
            R::InspectIntentions { definition_id } => O::Intentions {
                occurrences: self.store.intentions(auth, definition_id).await?,
            },
            R::CancelIntention {
                occurrence_id,
                reason,
            } => O::Intention {
                occurrence: Box::new(
                    self.store
                        .cancel_intention(auth, occurrence_id, reason)
                        .await?,
                ),
            },
            R::ConfirmIntention { occurrence_id } => O::Intention {
                occurrence: Box::new(self.store.confirm_intention(auth, occurrence_id).await?),
            },
            R::SweepIntentions { limit } => O::IntentionSweep {
                result: self.store.sweep_intentions(auth, limit).await?,
            },
            R::Recover => O::Done {
                affected: u64::from(self.store.recover_jobs(auth).await?),
            },
            R::PruneHistory => O::Done {
                affected: self.store.prune_job_history(auth).await?,
            },
        })
    }
}
