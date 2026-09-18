use crate::Host;
use memory_domain::{contracts::*, coordination::*, judgement::*, maintenance::*, records::*};
use memory_store::{Error, Result, maintenance::MaintenanceDisposition};
use std::collections::BTreeMap;
use uuid::Uuid;

pub(crate) fn family(id: &str) -> Result<JudgementDefinition> {
    let catalogue: Vec<JudgementDefinition> = serde_json::from_str(include_str!(
        "../../../packages/judgement/src/catalogue-data.json"
    ))?;
    catalogue
        .into_iter()
        .find(|d| d.id == id)
        .ok_or(Error::Forbidden)
}
impl Host {
    /// Bind a judgement to the canonical review artifact, definition and individual fields.
    pub(crate) async fn checked_choice(
        &self,
        a: &Authority,
        f: &Fence,
        (artifact, content): (Uuid, &serde_json::Value),
        decision: Option<&Uuid>,
        definition: &JudgementDefinition,
        fields: BTreeMap<String, String>,
    ) -> Result<Option<String>> {
        let Some(id) = decision else {
            return Ok(None);
        };
        let d = self.store.judgement_decision(a, f, *id).await?;
        if !d.inconsistencies.is_empty() {
            return Ok(None);
        }
        let packet = self.read_judgement_packet(a, d.packet_id).await?;
        self.store.check_judgement_packet(a, f, &packet).await?;
        if packet.local_check_id.is_some()
            || packet.questions.len() != 1
            || serde_json::to_value(&packet.questions[0].definition)?
                != serde_json::to_value(definition)?
            || packet.evidence.len() != fields.len()
        {
            return Err(Error::Forbidden);
        }
        for (name, pointer) in fields {
            let item = packet
                .evidence
                .iter()
                .find(|e| e.name == name)
                .ok_or(Error::Forbidden)?;
            if item.artifact_id != artifact
                || item.pointer != pointer
                || content.pointer(&pointer) != Some(&item.content)
            {
                return Err(Error::Forbidden);
            }
        }
        let Some(id) = d.selected_assessment else {
            return Ok(None);
        };
        let assessment = self.store.assessment(a, id).await?;
        assessment.validate(&packet).map_err(Error::Invalid)?;
        Ok(
            match assessment.answers.get(&format!(
                "{}.{}",
                definition.id, definition.questions[0].key
            )) {
                Some(JudgementAnswer::Choice { choice, .. }) => Some(choice.clone()),
                _ => None,
            },
        )
    }
    pub(crate) async fn review_maintenance(
        &self,
        a: &Authority,
        f: &Fence,
        id: Uuid,
        request: MaintenanceRequest,
    ) -> Result<MaintenanceReview> {
        let r = self.store.maintenance_review(a, f, id, request).await?;
        self.publish_work_artifact(
            a,
            f,
            id,
            "Maintenance evidence review".into(),
            serde_json::to_string(&r)?,
            vec![],
        )
        .await?;
        Ok(r)
    }
    pub(crate) async fn commit_maintenance(
        &self,
        a: &Authority,
        f: &Fence,
        request: MaintenanceCommit,
    ) -> Result<MaintenanceResult> {
        let r = self
            .store
            .load_maintenance_review(a, f, request.review_id)
            .await?;
        let content = serde_json::to_value(&r)?;
        let classification = if !r.request.removed_sources.is_empty() {
            Some("support_removal".into())
        } else {
            self.checked_choice(
                a,
                f,
                (r.id, &content),
                request.decisions.get("J13"),
                &family("J13")?,
                fields(&[
                    ("before", "/before"),
                    ("after", "/request/after"),
                    ("valid_time", "/request/after/valid_time"),
                ]),
            )
            .await?
        };
        let kind = match classification.as_deref() {
            Some("wording_only") => ChangeKind::Wording,
            Some("correction") => ChangeKind::Correction,
            Some("new_valid_time_state") => ChangeKind::WorldChange,
            Some("support_removal") => ChangeKind::SupportRemoval,
            _ => ChangeKind::Unresolved,
        };
        let mut d = MaintenanceDisposition {
            kind,
            comparison: None,
            support: BTreeMap::new(),
            indirect: BTreeMap::new(),
            conflicts: BTreeMap::new(),
        };
        if r.request.after.is_some() {
            d.comparison = self
                .checked_choice(
                    a,
                    f,
                    (r.id, &content),
                    request.decisions.get("J14/proposal"),
                    &family("J14")?,
                    fields(&[("claims", "/request"), ("scope_and_time", "/before")]),
                )
                .await?;
        }
        for (i, s) in r.affected.iter().enumerate() {
            if let Some(choice) = self
                .checked_choice(
                    a,
                    f,
                    (r.id, &content),
                    request.decisions.get(&format!("J15/{i}")),
                    &family("J15")?,
                    fields(&[
                        ("claim", &format!("/affected/{i}/claim")),
                        ("remaining_evidence", &format!("/affected/{i}")),
                        ("source_lineage", &format!("/affected/{i}/source_groups")),
                    ]),
                )
                .await?
            {
                d.support.insert(s.claim.reference.memory_id, choice);
            }
        }
        for (i, m) in r.indirect.iter().enumerate() {
            if let Some(choice) = self
                .checked_choice(
                    a,
                    f,
                    (r.id, &content),
                    request.decisions.get(&format!("J16/{i}")),
                    &family("J16")?,
                    fields(&[
                        ("change", "/request"),
                        ("dependency", &format!("/indirect/{i}")),
                    ]),
                )
                .await?
            {
                d.indirect.insert(m.reference.memory_id, choice);
            }
        }
        for (i, c) in r.conflicts.iter().enumerate() {
            if let Some(choice) = self
                .checked_choice(
                    a,
                    f,
                    (r.id, &content),
                    request.decisions.get(&format!("J14/{i}")),
                    &family("J14")?,
                    fields(&[
                        ("claims", &format!("/conflicts/{i}")),
                        ("scope_and_time", "/before"),
                    ]),
                )
                .await?
            {
                d.conflicts.insert(c.relation.id, choice);
            }
        }
        self.store.commit_maintenance(a, f, &r, d).await
    }
    pub(crate) async fn maintenance_scope(&self, a: &Authority, f: &Fence) -> Result<Authority> {
        let job = self.store.assigned_job(a, f).await?;
        if job.cancel_requested {
            return Err(Error::Forbidden);
        }
        Ok(Authority {
            scope: job.spec.brief.scope,
            ..a.clone()
        })
    }
    pub(crate) async fn current_memories(
        &self,
        a: &Authority,
        f: &Fence,
        references: Vec<MemoryRef>,
    ) -> Result<Vec<MemoryVersion>> {
        if references.len() > 100 {
            return Err(Error::LimitExceeded);
        }
        let a = self.maintenance_scope(a, f).await?;
        let mut result = vec![];
        for r in references {
            // Resolve the current revision without substituting it into a task's recorded evidence.
            match self.store.current_memory(&a, r.memory_id).await {
                Ok(m) => result.push(m),
                Err(Error::NotFound | Error::Unavailable) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(result)
    }
}
pub(crate) fn fields(items: &[(&str, &str)]) -> BTreeMap<String, String> {
    items
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}
