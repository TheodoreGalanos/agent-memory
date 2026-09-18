//! Bounded exact retrieval; every scoring input has passed the domain scope query.
use crate::{Error, Result, Store, access::require_write_scope};
use memory_domain::{
    activation::*,
    contracts::{Authority, MemoryRef},
    records::*,
};
use sqlx::{AnyConnection, Row};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use uuid::Uuid;

pub fn representation(record: &RecordDraft) -> Result<String> {
    Ok(format!(
        "{}\n{}",
        record.label,
        serde_json::to_string(&record.content)?
    ))
}
pub(crate) async fn index_record(
    connection: &mut AnyConnection,
    tenant: &str,
    version: &str,
    sequence: i64,
    record: &RecordDraft,
) -> Result<()> {
    sqlx::query("INSERT INTO memory_search (version_id,document) VALUES ($1,$2)")
        .bind(version)
        .bind(representation(record)?)
        .execute(&mut *connection)
        .await?;
    let mut entities = record.scope.entity_ids.clone();
    if let MemoryContent::Knowledge {
        subject: Some(id), ..
    } = &record.content
    {
        entities.push(*id);
    }
    for entity in entities {
        sqlx::query("INSERT INTO search_entities (version_id,entity_id) VALUES ($1,$2) ON CONFLICT DO NOTHING")
            .bind(version).bind(entity.to_string()).execute(&mut *connection).await?;
    }
    sqlx::query("INSERT INTO search_updates (tenant_id,version_id,sequence) VALUES ($1,$2,$3)")
        .bind(tenant)
        .bind(version)
        .bind(sequence)
        .execute(connection)
        .await?;
    Ok(())
}
fn key(r: &MemoryRef) -> (Uuid, u32) {
    (r.memory_id, r.revision.get())
}
fn eligible(m: &MemoryVersion, q: &ActivationQuery, cutoff: u32) -> bool {
    m.recorded.sequence <= cutoff
        && m.recorded_until.is_none_or(|until| cutoff < until)
        && m.record.availability == Availability::Routine
        && q.valid_at.is_none_or(|t| match &m.record.valid_time {
            ValidTime::Unknown => false,
            ValidTime::Interval { from, to } => {
                from.is_none_or(|v| v <= t) && to.is_none_or(|v| t < v)
            }
        })
}
fn conditions(m: &MemoryVersion) -> Vec<String> {
    if let MemoryContent::Procedure {
        applicability,
        exclusions,
        ..
    } = &m.record.content
    {
        applicability
            .iter()
            .cloned()
            .chain(
                exclusions
                    .iter()
                    .map(|e| format!("This exclusion does not apply: {e}")),
            )
            .collect()
    } else {
        vec![]
    }
}
fn cosine(a: &[f64], b: &[f64]) -> f64 {
    let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    dot / (a.iter().map(|v| v * v).sum::<f64>().sqrt()
        * b.iter().map(|v| v * v).sum::<f64>().sqrt())
}
impl Store {
    /// Provider embeddings are supplied explicitly; this method never calls a paid service.
    pub async fn save_embedding(
        &self,
        auth: &Authority,
        reference: &MemoryRef,
        embedding: Embedding,
    ) -> Result<()> {
        embedding.validate().map_err(Error::Invalid)?;
        let (mut tx, _) = self.begin_write().await?;
        require_write_scope(&mut tx, auth, reference.memory_id).await?;
        let memory = crate::memories::memory_version(&mut tx, auth, reference).await?;
        if memory.recorded_until.is_some() || memory.record.availability != Availability::Routine {
            return Err(Error::Conflict);
        }
        sqlx::query("INSERT INTO memory_embeddings (tenant_id,version_id,data) VALUES ($1,$2,$3) ON CONFLICT (tenant_id,version_id) DO UPDATE SET data=excluded.data")
            .bind(auth.tenant_id.to_string()).bind(memory.version_id.to_string()).bind(serde_json::to_string(&embedding)?).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn activation(
        &self,
        auth: &Authority,
        id: Uuid,
        query: ActivationQuery,
        cursor: Option<ActivationCursor>,
    ) -> Result<ActivationWindow> {
        query.validate().map_err(Error::Invalid)?;
        if !auth.scope.permits(&query.scope) {
            return Err(Error::Forbidden);
        }
        let narrow = Authority {
            scope: query.scope.clone(),
            ..auth.clone()
        };
        let now = self.position().await?;
        let cutoff = cursor
            .as_ref()
            .map(|c| c.cutoff)
            .or(query.recorded_as_of)
            .unwrap_or(now);
        if cutoff > now
            || cursor.as_ref().is_some_and(|c| {
                serde_json::to_value(&*c.query).ok() != serde_json::to_value(&query).ok()
            })
        {
            return Err(Error::Invalid(
                "Activation cursor belongs to another query or future snapshot".into(),
            ));
        }
        for entity in &query.entities {
            self.entity(&narrow, *entity).await?;
        }
        let records = self
            .memories(
                &narrow,
                &MemoryQuery {
                    recorded_as_of: Some(cutoff),
                    valid_at: query.valid_at,
                    after_id: cursor.as_ref().map(|c| c.after_id),
                    limit: query.scan_limit + 1,
                    ..Default::default()
                },
            )
            .await?;
        let more = records.len() > usize::from(query.scan_limit);
        let page: Vec<_> = records.into_iter().take(query.scan_limit.into()).collect();
        let next = if more {
            page.last().map(|m| ActivationCursor {
                after_id: m.reference.memory_id,
                cutoff,
                query: Box::new(query.clone()),
            })
        } else {
            None
        };
        let mut records = Vec::new();
        for m in page {
            if (!query.families.is_empty()
                && !query
                    .families
                    .iter()
                    .any(|f| f == m.record.content.family()))
                || query
                    .fresh_after
                    .is_some_and(|t| m.recorded.recorded_at < t)
            {
                continue;
            }
            match self
                .activation_memory(&narrow, &m.reference, &query, cutoff)
                .await
            {
                Ok(m) => records.push(m),
                Err(Error::NotFound) => (),
                Err(e) => return Err(e),
            }
        }
        let mut coverage = vec![
            "Direct scoped domain scan; lexical projection updated in the record transaction"
                .into(),
        ];
        if more {
            coverage.push("Scan bound reached; continue with the returned cursor".into());
        }
        let identities = self.identity_matches(&narrow, &query, cutoff).await?;
        if identities.len() > query.scan_limit.into() {
            coverage.push("Identity lookup bound reached; narrow the entity query".into());
        }
        for m in identities.into_iter().take(query.scan_limit.into()) {
            if records.iter().any(|r| r.version_id == m.version_id) {
                continue;
            }
            if (!query.families.is_empty()
                && !query
                    .families
                    .iter()
                    .any(|f| f == m.record.content.family()))
                || query
                    .fresh_after
                    .is_some_and(|t| m.recorded.recorded_at < t)
            {
                continue;
            }
            match self
                .activation_memory(&narrow, &m.reference, &query, cutoff)
                .await
            {
                Ok(m) => records.push(m),
                Err(Error::NotFound) => (),
                Err(e) => return Err(e),
            }
        }
        let lexical = self.lexical(&records, &query.question).await?;
        let mut ranked: BTreeMap<Uuid, ActivationCandidate> = BTreeMap::new();
        let mut channels: Vec<Vec<(Uuid, f64)>> = vec![vec![], vec![], vec![]];
        let mut missing_vectors = 0;
        for memory in &records {
            let entity_match = memory
                .record
                .scope
                .entity_ids
                .iter()
                .any(|id| query.entities.contains(id))
                || matches!(&memory.record.content, MemoryContent::Knowledge { subject: Some(id), .. } if query.entities.contains(id));
            if query.exact.contains(&memory.reference.memory_id) || entity_match {
                channels[0].push((memory.version_id, 1.0));
            }
            if let Some(score) = lexical.get(&memory.version_id) {
                channels[1].push((memory.version_id, *score));
            }
            if let Some(vector) = &query.vector {
                let row = sqlx::query(
                    "SELECT data FROM memory_embeddings WHERE tenant_id=$1 AND version_id=$2",
                )
                .bind(auth.tenant_id.to_string())
                .bind(memory.version_id.to_string())
                .fetch_optional(&self.pool)
                .await?;
                if let Some(row) = row {
                    let stored: Embedding = serde_json::from_str(row.try_get("data")?)?;
                    if stored.identity == vector.identity {
                        let score = cosine(&stored.values, &vector.values);
                        if score > 0.0 {
                            channels[2].push((memory.version_id, score));
                        }
                    } else {
                        missing_vectors += 1;
                    }
                } else {
                    missing_vectors += 1;
                }
            }
        }
        for (index, channel) in channels.iter_mut().enumerate() {
            channel.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
            for (rank, (id, _)) in channel.iter().enumerate() {
                let memory = records
                    .iter()
                    .find(|m| m.version_id == *id)
                    .expect("scoped scoring input");
                let candidate = ranked.entry(*id).or_insert_with(|| ActivationCandidate {
                    memory: memory.clone(),
                    score: 0.0,
                    channels: vec![],
                    conditions: conditions(memory),
                    definition: None,
                });
                // Explicit identity outranks lexical/vector similarity. Other channels use RRF.
                candidate.score += if index == 0 {
                    1.0
                } else {
                    1.0 / (60.0 + rank as f64 + 1.0)
                };
                candidate
                    .channels
                    .push(["exact_entity", "lexical", "vector"][index].into());
            }
        }
        if missing_vectors > 0 {
            coverage.push(format!("{missing_vectors} eligible records lack a matching current embedding; lexical/entity retrieval still covers this page"));
        }
        let mut seeds: Vec<_> = ranked.into_values().collect();
        seeds.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then(key(&a.memory.reference).cmp(&key(&b.memory.reference)))
        });
        if seeds.len() > query.candidate_limit.into() {
            coverage.push("Candidate bound reached; lower-ranked candidates remain inspectable through narrower queries".into());
        }
        seeds.truncate(query.candidate_limit.into());
        let mut candidates = seeds.clone();
        let mut groups: Vec<ContextGroup> = vec![];
        for seed in seeds {
            if groups.iter().any(|g| {
                g.members
                    .iter()
                    .any(|r| key(r) == key(&seed.memory.reference))
            }) {
                continue;
            }
            let mut pending = VecDeque::from([seed.memory.reference.clone()]);
            let mut group = ContextGroup {
                members: vec![],
                relations: vec![],
                unresolved_conflict: false,
                complete: true,
            };
            let mut visited = BTreeSet::new();
            while let Some(reference) = pending.pop_front() {
                if !visited.insert(key(&reference)) {
                    continue;
                }
                if visited.len() > query.traversal_limit.into() {
                    group.complete = false;
                    break;
                }
                let memory = match self
                    .activation_memory(&narrow, &reference, &query, cutoff)
                    .await
                {
                    Ok(m) if eligible(&m, &query, cutoff) => m,
                    Ok(_) | Err(Error::NotFound) => {
                        group.complete = false;
                        continue;
                    }
                    Err(e) => return Err(e),
                };
                group.members.push(reference.clone());
                if !candidates
                    .iter()
                    .any(|c| key(&c.memory.reference) == key(&reference))
                {
                    candidates.push(ActivationCandidate {
                        conditions: conditions(&memory),
                        definition: None,
                        memory: memory.clone(),
                        score: seed.score,
                        channels: vec!["relation".into()],
                    });
                }
                if let MemoryContent::Procedure {
                    counterexamples, ..
                } = &memory.record.content
                {
                    pending.extend(counterexamples.clone());
                }
                let relations = self
                    .relations(
                        &narrow,
                        reference.memory_id,
                        &MemoryQuery {
                            recorded_as_of: Some(cutoff),
                            valid_at: query.valid_at,
                            limit: query.traversal_limit + 1,
                            ..Default::default()
                        },
                    )
                    .await?;
                if relations.len() > query.traversal_limit.into() {
                    group.complete = false;
                }
                for relation in relations.into_iter().take(query.traversal_limit.into()) {
                    if ![
                        RelationKind::Supports,
                        RelationKind::Challenges,
                        RelationKind::ConflictsWith,
                        RelationKind::DerivedFrom,
                        RelationKind::DependsOn,
                    ]
                    .contains(&relation.relation.kind)
                    {
                        continue;
                    }
                    let r = &relation.relation;
                    let endpoint = if key(&r.from) == key(&reference) {
                        &r.to
                    } else if key(&r.to) == key(&reference) {
                        &r.from
                    } else {
                        continue;
                    };
                    group.unresolved_conflict |=
                        [RelationKind::Challenges, RelationKind::ConflictsWith].contains(&r.kind);
                    pending.push_back(endpoint.clone());
                    if !group.relations.iter().any(|v| v.id == relation.id) {
                        group.relations.push(relation);
                    }
                }
            }
            if !group.complete {
                coverage.push("Related material is incomplete or outside the query; this group cannot be selected as complete".into());
            }
            // Join overlapping groups so a shared exception can never be budgeted separately.
            let mut i = 0;
            while i < groups.len() {
                if groups[i]
                    .members
                    .iter()
                    .any(|r| group.members.iter().any(|m| key(r) == key(m)))
                {
                    let other = groups.remove(i);
                    for m in other.members {
                        if !group.members.iter().any(|r| key(r) == key(&m)) {
                            group.members.push(m);
                        }
                    }
                    for r in other.relations {
                        if !group.relations.iter().any(|v| v.id == r.id) {
                            group.relations.push(r);
                        }
                    }
                    group.complete &= other.complete;
                    group.unresolved_conflict |= other.unresolved_conflict;
                    i = 0;
                } else {
                    i += 1;
                }
            }
            groups.push(group);
        }
        Ok(ActivationWindow {
            id,
            job_id: None,
            query,
            cursor,
            cutoff,
            projection_watermark: cutoff,
            eligible: records.iter().map(|m| m.reference.clone()).collect(),
            candidates,
            groups,
            next,
            coverage,
        })
    }

    async fn identity_matches(
        &self,
        auth: &Authority,
        query: &ActivationQuery,
        cutoff: u32,
    ) -> Result<Vec<MemoryVersion>> {
        if query.exact.is_empty() && query.entities.is_empty() {
            return Ok(vec![]);
        }
        let (predicate, mut values) = crate::access::predicate(auth);
        let mut clauses = vec![];
        for (ids, entity) in [(&query.exact, false), (&query.entities, true)] {
            if ids.is_empty() {
                continue;
            }
            let mut parameters = vec![];
            for id in ids {
                values.push(id.to_string());
                parameters.push(format!("${}", values.len()));
            }
            let list = parameters.join(",");
            clauses.push(if entity {format!("EXISTS (SELECT 1 FROM search_entities e WHERE e.version_id=v.version_id AND e.entity_id IN ({list}))")} else {format!("v.id IN ({list})")});
        }
        let mut sql = format!(
            "SELECT v.*,m.created_by FROM memory_versions v JOIN memory_records m ON m.tenant_id=v.tenant_id AND m.id=v.id JOIN resource_scopes s ON s.tenant_id=v.tenant_id AND s.id=v.id WHERE {predicate} AND v.availability='routine' AND ({})",
            clauses.join(" OR ")
        );
        crate::temporal::filter(
            &mut sql,
            &mut values,
            &MemoryQuery {
                recorded_as_of: Some(cutoff),
                valid_at: query.valid_at,
                ..Default::default()
            },
        );
        sql.push_str(&format!(" ORDER BY v.id LIMIT {}", query.scan_limit + 1));
        let mut statement = sqlx::query(&sql);
        for value in values {
            statement = statement.bind(value);
        }
        statement
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(crate::memories::decode_memory)
            .collect()
    }

    async fn lexical(
        &self,
        records: &[MemoryVersion],
        question: &str,
    ) -> Result<BTreeMap<Uuid, f64>> {
        let tokens: Vec<_> = question
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
            .take(32)
            .map(str::to_lowercase)
            .collect();
        if records.is_empty() || tokens.is_empty() {
            return Ok(BTreeMap::new());
        }
        let expression = if self.postgres {
            tokens.join(" | ")
        } else {
            tokens
                .iter()
                .map(|t| format!("\"{t}\""))
                .collect::<Vec<_>>()
                .join(" OR ")
        };
        let parameters = (2..records.len() + 2)
            .map(|i| format!("${i}"))
            .collect::<Vec<_>>()
            .join(",");
        let sql = if self.postgres {
            format!(
                "SELECT version_id FROM memory_search WHERE version_id IN ({parameters}) AND to_tsvector('simple',document) @@ to_tsquery('simple',$1)"
            )
        } else {
            format!(
                "SELECT version_id FROM memory_search WHERE version_id IN ({parameters}) AND memory_search MATCH $1"
            )
        };
        let mut statement = sqlx::query(&sql).bind(expression);
        for record in records {
            statement = statement.bind(record.version_id.to_string());
        }
        let rows = statement.fetch_all(&self.pool).await?;
        let mut scores = BTreeMap::new();
        for row in rows {
            let id: Uuid = row
                .try_get::<String, _>("version_id")?
                .parse()
                .map_err(|_| Error::Invalid("Search version ID".into()))?;
            let record = records
                .iter()
                .find(|m| m.version_id == id)
                .expect("permitted search ID");
            let text = representation(&record.record)?.to_lowercase();
            // No global corpus statistics: another tenant cannot affect these scores.
            scores.insert(
                id,
                tokens
                    .iter()
                    .filter(|t| {
                        text.split(|c: char| !c.is_alphanumeric())
                            .any(|s| s == t.as_str())
                    })
                    .count() as f64,
            );
        }
        Ok(scores)
    }
}

impl Store {
    pub async fn activation_memory(
        &self,
        auth: &Authority,
        reference: &MemoryRef,
        query: &ActivationQuery,
        cutoff: u32,
    ) -> Result<MemoryVersion> {
        let memory = self.memory(auth, reference).await?;
        let mut references = memory.record.derived_from.clone();
        if let MemoryContent::Procedure {
            counterexamples, ..
        } = &memory.record.content
        {
            references.extend(counterexamples.clone());
        }
        if let Qualification::Evaluated { evidence, .. } = &memory.record.qualification {
            references.extend(evidence.clone());
        }
        for linked in references {
            self.memory(auth, &linked).await?;
        }
        let row = sqlx::query("SELECT revision,availability FROM memory_versions WHERE tenant_id=$1 AND id=$2 AND recorded_to IS NULL")
            .bind(auth.tenant_id.to_string()).bind(reference.memory_id.to_string()).fetch_one(&self.pool).await?;
        let current_revision: i64 = row.try_get("revision")?;
        let availability: String = row.try_get("availability")?;
        if availability != "routine"
            || (query.recorded_as_of.is_none()
                && current_revision != i64::from(reference.revision.get()))
            || !eligible(&memory, query, cutoff)
        {
            return Err(Error::NotFound);
        }
        Ok(memory)
    }
    pub async fn activation_window(
        &self,
        auth: &Authority,
        permit: &memory_domain::coordination::Fence,
        id: Uuid,
    ) -> Result<Option<ActivationWindow>> {
        self.assigned_job(auth, permit).await?;
        let row = sqlx::query(
            "SELECT data FROM activation_windows WHERE tenant_id=$1 AND job_id=$2 AND id=$3",
        )
        .bind(auth.tenant_id.to_string())
        .bind(permit.job_id.to_string())
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.map(|r| serde_json::from_str(r.get("data")).map_err(Error::from))
            .transpose()
    }
    pub async fn save_activation_window(
        &self,
        auth: &Authority,
        permit: &memory_domain::coordination::Fence,
        window: &ActivationWindow,
    ) -> Result<()> {
        let (mut tx, at) = self.begin_write().await?;
        let job = crate::coordinator::fence(&mut tx, auth, permit, at.recorded_at).await?;
        if job.cancel_requested || job.deadline <= at.recorded_at || window.job_id != Some(job.id) {
            return Err(Error::Conflict);
        }
        sqlx::query("INSERT INTO activation_windows(tenant_id,id,job_id,data) VALUES($1,$2,$3,$4)")
            .bind(auth.tenant_id.to_string())
            .bind(window.id.to_string())
            .bind(job.id.to_string())
            .bind(serde_json::to_string(window)?)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}

impl Store {
    /// Consume scoped index-update events. The caller supplies embeddings for the returned
    /// representation and resumes after the last cursor; stale versions are never handed out.
    pub async fn embedding_inputs(
        &self,
        auth: &Authority,
        after: Option<SearchCursor>,
        limit: u16,
    ) -> Result<Vec<EmbeddingInput>> {
        if !(1..=100).contains(&limit) {
            return Err(Error::LimitExceeded);
        }
        let (predicate, mut values) = crate::access::predicate(auth);
        let mut sql = format!(
            "SELECT v.id,v.revision,v.data,u.sequence,u.version_id FROM search_updates u JOIN memory_versions v ON v.tenant_id=u.tenant_id AND v.version_id=u.version_id JOIN resource_scopes s ON s.tenant_id=v.tenant_id AND s.id=v.id WHERE {predicate} AND v.recorded_to IS NULL AND v.availability='routine'"
        );
        if let Some(after) = after {
            values.push(after.sequence.to_string());
            let sequence = values.len();
            values.push(after.version_id.to_string());
            let version = values.len();
            sql.push_str(&format!(" AND (u.sequence > CAST(${sequence} AS BIGINT) OR (u.sequence=CAST(${sequence} AS BIGINT) AND u.version_id>${version}))"));
        }
        sql.push_str(&format!(" ORDER BY u.sequence,u.version_id LIMIT {limit}"));
        let mut statement = sqlx::query(&sql);
        for value in values {
            statement = statement.bind(value);
        }
        let rows = statement.fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                let record: RecordDraft = serde_json::from_str(row.try_get("data")?)?;
                Ok(EmbeddingInput {
                    memory: MemoryRef {
                        memory_id: crate::database::id(row.try_get("id")?)?,
                        revision: crate::database::number(row.try_get("revision")?)?
                            .try_into()
                            .map_err(|_| Error::Invalid("Revision".into()))?,
                        label: record.label.clone(),
                    },
                    text: representation(&record)?,
                    cursor: SearchCursor {
                        sequence: crate::database::number(row.try_get("sequence")?)?,
                        version_id: crate::database::id(row.try_get("version_id")?)?,
                    },
                })
            })
            .collect()
    }
}
