CREATE TABLE deletion_jobs (
 tenant_id TEXT NOT NULL,id TEXT NOT NULL,data TEXT NOT NULL,
 PRIMARY KEY(tenant_id,id)
);
CREATE TABLE revoked_resources (
 tenant_id TEXT NOT NULL,resource_id TEXT NOT NULL,epoch BIGINT NOT NULL,
 PRIMARY KEY(tenant_id,resource_id)
);
CREATE TABLE resource_exposures (
 tenant_id TEXT NOT NULL,job_id TEXT NOT NULL,resource_id TEXT NOT NULL,
 PRIMARY KEY(tenant_id,job_id,resource_id)
);
CREATE INDEX exposure_resource ON resource_exposures(tenant_id,resource_id);
CREATE TABLE deleted_sessions (
 tenant_id TEXT NOT NULL,session_id TEXT NOT NULL,
 PRIMARY KEY(tenant_id,session_id)
);
