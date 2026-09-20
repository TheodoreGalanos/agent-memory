CREATE TABLE job_experiences (
 tenant_id TEXT NOT NULL, job_id TEXT NOT NULL, source_id TEXT NOT NULL, revision TEXT NOT NULL,
 published_at BIGINT NOT NULL, formation_job_id TEXT,
 PRIMARY KEY (tenant_id, job_id), FOREIGN KEY (tenant_id, job_id) REFERENCES jobs(tenant_id, id)
);
