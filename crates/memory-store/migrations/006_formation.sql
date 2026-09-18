CREATE TABLE formation_cursors (
    tenant_id TEXT NOT NULL, source_id TEXT NOT NULL, source_revision TEXT NOT NULL,
    operation TEXT NOT NULL, policy_id TEXT NOT NULL, policy_revision BIGINT NOT NULL,
    cursor BIGINT NOT NULL,
    PRIMARY KEY(tenant_id,source_id,source_revision,operation,policy_id,policy_revision)
);
CREATE TABLE formation_results (
    tenant_id TEXT NOT NULL, window_id TEXT NOT NULL, job_id TEXT NOT NULL,
    request TEXT NOT NULL, result TEXT NOT NULL,
    PRIMARY KEY(tenant_id,window_id)
);
CREATE TABLE formation_events (
    tenant_id TEXT NOT NULL, source_id TEXT NOT NULL, operation TEXT NOT NULL,
    policy_id TEXT NOT NULL, policy_revision BIGINT NOT NULL, event_id TEXT NOT NULL,
    event TEXT NOT NULL, records TEXT NOT NULL, deferred TEXT NOT NULL,
    PRIMARY KEY(tenant_id,source_id,operation,policy_id,policy_revision,event_id)
);
CREATE TABLE formation_windows (
    tenant_id TEXT NOT NULL, id TEXT NOT NULL, job_id TEXT NOT NULL,
    data TEXT NOT NULL,
    PRIMARY KEY(tenant_id,id)
);
