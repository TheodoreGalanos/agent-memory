CREATE TABLE semantic_assessments (
    tenant_id TEXT NOT NULL, id TEXT NOT NULL, job_id TEXT NOT NULL,
    packet_id TEXT NOT NULL, expires_at BIGINT NOT NULL, data TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id)
);
CREATE TABLE judgement_decisions (
    tenant_id TEXT NOT NULL, id TEXT NOT NULL, job_id TEXT NOT NULL,
    data TEXT NOT NULL, PRIMARY KEY (tenant_id, id)
);
CREATE TABLE task_local_checks (
    tenant_id TEXT NOT NULL, id TEXT NOT NULL, job_id TEXT NOT NULL,
    data TEXT NOT NULL, PRIMARY KEY (tenant_id, id)
);
