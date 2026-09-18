CREATE TABLE consolidation_windows (
    tenant_id TEXT NOT NULL, id TEXT NOT NULL, job_id TEXT NOT NULL, data TEXT NOT NULL,
    PRIMARY KEY(tenant_id,id)
);
CREATE TABLE consolidation_reviews (
    tenant_id TEXT NOT NULL, id TEXT NOT NULL, job_id TEXT NOT NULL, data TEXT NOT NULL, result TEXT,
    PRIMARY KEY(tenant_id,id)
);
CREATE TABLE qualification_adoptions (
    tenant_id TEXT NOT NULL, job_id TEXT NOT NULL, data TEXT NOT NULL,
    PRIMARY KEY(tenant_id,job_id)
);
