CREATE TABLE scheduled_work (
 tenant_id TEXT NOT NULL, trigger_kind TEXT NOT NULL, trigger_id TEXT NOT NULL, dedupe_key TEXT NOT NULL,
 job_id TEXT, unschedulable TEXT, causal_origin_job_id TEXT, scheduled_at BIGINT NOT NULL,
 PRIMARY KEY (tenant_id, trigger_kind, trigger_id)
);
CREATE INDEX scheduled_work_dedupe ON scheduled_work (tenant_id, trigger_kind, dedupe_key);
CREATE TABLE consolidation_cohort_members (
 tenant_id TEXT NOT NULL, policy_id TEXT NOT NULL, memory_id TEXT NOT NULL, cohort_id TEXT NOT NULL,
 PRIMARY KEY (tenant_id, policy_id, memory_id)
);
ALTER TABLE memory_changes ADD COLUMN causal_job_id TEXT;
