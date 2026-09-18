CREATE TABLE job_child_links (
    tenant_id TEXT NOT NULL,
    parent_id TEXT NOT NULL,
    child_id TEXT NOT NULL,
    waiting BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (tenant_id, parent_id, child_id)
);
CREATE INDEX child_link_lookup ON job_child_links(tenant_id, child_id);
CREATE TABLE job_artifacts (
    tenant_id TEXT NOT NULL,
    job_id TEXT NOT NULL,
    artifact_id TEXT NOT NULL,
    PRIMARY KEY (tenant_id, job_id, artifact_id)
);
