use crate::{
    Error, Result, Store,
    access::{read_scope, require_write_scope, same_scope, write_scope},
    database::{changed, current_revision},
};
use memory_domain::{
    contracts::{Authority, ConfigRef, Scope},
    records::{MemoryPolicy, PolicyVersion, ValidTime},
};
use sqlx::{AnyConnection, Row};
use uuid::Uuid;

impl Store {
    pub async fn create_policy(
        &self,
        authority: &Authority,
        label: &str,
        scope: Scope,
        policy: MemoryPolicy,
        effective: ValidTime,
    ) -> Result<PolicyVersion> {
        let (mut tx, recorded) = self.begin_write().await?;
        let result = create_policy_in(
            &mut tx, authority, label, scope, policy, effective, recorded,
        )
        .await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn revise_policy(
        &self,
        authority: &Authority,
        expected: &ConfigRef,
        policy: MemoryPolicy,
        effective: ValidTime,
    ) -> Result<PolicyVersion> {
        let (mut tx, recorded) = self.begin_write().await?;
        let result =
            revise_policy_in(&mut tx, authority, expected, policy, effective, recorded).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn policy(
        &self,
        authority: &Authority,
        reference: &ConfigRef,
    ) -> Result<PolicyVersion> {
        policy_version(&mut *self.pool.acquire().await?, authority, reference).await
    }
}

fn validate(label: &str, policy: &MemoryPolicy, effective: &ValidTime) -> Result<()> {
    if policy.semantic_triggers.len() > 16 {
        return Err(Error::LimitExceeded);
    }
    let mut ids = std::collections::BTreeSet::new();
    for d in &policy.semantic_triggers {
        d.validate().map_err(Error::Invalid)?;
        if !ids.insert(&d.id)
            || d.questions.len() != 1
            || d.input_requirements != ["intention", "event"]
            || !d.permitted_uses.iter().any(|u| u == "candidate_intentions")
        {
            return Err(Error::Invalid(
                "Invalid established semantic trigger definition".into(),
            ));
        }
    }
    effective.validate().map_err(Error::Invalid)?;
    if let Some(rules) = &policy.consolidation {
        rules.validate().map_err(Error::Invalid)?;
    }
    if label.trim().is_empty() || policy.retention_purpose.trim().is_empty() {
        return Err(Error::Invalid(
            "Policy needs a label and retention purpose".into(),
        ));
    }
    Ok(())
}

async fn insert_policy(
    connection: &mut AnyConnection,
    authority: &Authority,
    version: &PolicyVersion,
) -> Result<()> {
    sqlx::query("INSERT INTO policy_versions (tenant_id,id,revision,data) VALUES ($1,$2,$3,$4)")
        .bind(authority.tenant_id.to_string())
        .bind(version.reference.id.to_string())
        .bind(i64::from(version.reference.revision.get()))
        .bind(serde_json::to_string(version)?)
        .execute(connection)
        .await?;
    Ok(())
}

pub(crate) async fn policy_version(
    connection: &mut AnyConnection,
    authority: &Authority,
    reference: &ConfigRef,
) -> Result<PolicyVersion> {
    let scope = read_scope(connection, authority, reference.id).await?;
    let row = sqlx::query(
        "SELECT data FROM policy_versions WHERE tenant_id=$1 AND id=$2 AND revision=$3",
    )
    .bind(authority.tenant_id.to_string())
    .bind(reference.id.to_string())
    .bind(i64::from(reference.revision.get()))
    .fetch_optional(connection)
    .await?
    .ok_or(Error::NotFound)?;
    let result: PolicyVersion = serde_json::from_str(row.try_get("data")?)?;
    if !same_scope(&scope, &result.scope) {
        return Err(Error::Invalid("Stored policy scope is inconsistent".into()));
    }
    Ok(result)
}

pub(crate) async fn current_policy(
    connection: &mut AnyConnection,
    authority: &Authority,
    reference: &ConfigRef,
) -> Result<PolicyVersion> {
    let policy = policy_version(connection, authority, reference).await?;
    if current_revision(
        connection,
        "policies",
        &authority.tenant_id.to_string(),
        &reference.id.to_string(),
    )
    .await?
        != reference.revision.get()
    {
        return Err(Error::Conflict);
    }
    Ok(policy)
}

pub(crate) async fn create_policy_in(
    connection: &mut AnyConnection,
    authority: &Authority,
    label: &str,
    scope: Scope,
    policy: MemoryPolicy,
    effective: ValidTime,
    recorded: memory_domain::records::CommitPosition,
) -> Result<PolicyVersion> {
    validate(label, &policy, &effective)?;
    let id = Uuid::now_v7();
    write_scope(connection, authority, id, &scope).await?;
    let version = PolicyVersion {
        reference: ConfigRef {
            id,
            revision: 1.try_into().unwrap(),
            label: label.into(),
        },
        scope,
        policy,
        effective,
        recorded,
    };
    sqlx::query("INSERT INTO policies (tenant_id,id,label,revision) VALUES ($1,$2,$3,1)")
        .bind(authority.tenant_id.to_string())
        .bind(id.to_string())
        .bind(label)
        .execute(&mut *connection)
        .await?;
    insert_policy(connection, authority, &version).await?;
    Ok(version)
}
pub(crate) async fn revise_policy_in(
    connection: &mut AnyConnection,
    authority: &Authority,
    expected: &ConfigRef,
    policy: MemoryPolicy,
    effective: ValidTime,
    recorded: memory_domain::records::CommitPosition,
) -> Result<PolicyVersion> {
    validate(&expected.label, &policy, &effective)?;
    require_write_scope(connection, authority, expected.id).await?;
    let prior = policy_version(connection, authority, expected).await?;
    let revision = expected
        .revision
        .get()
        .checked_add(1)
        .ok_or(Error::LimitExceeded)?;
    changed(
        sqlx::query("UPDATE policies SET revision=$1 WHERE tenant_id=$2 AND id=$3 AND revision=$4")
            .bind(i64::from(revision))
            .bind(authority.tenant_id.to_string())
            .bind(expected.id.to_string())
            .bind(i64::from(expected.revision.get()))
            .execute(&mut *connection)
            .await?
            .rows_affected(),
    )?;
    let version = PolicyVersion {
        reference: ConfigRef {
            id: expected.id,
            revision: revision.try_into().unwrap(),
            label: prior.reference.label,
        },
        scope: prior.scope,
        policy,
        effective,
        recorded,
    };
    insert_policy(connection, authority, &version).await?;
    Ok(version)
}
