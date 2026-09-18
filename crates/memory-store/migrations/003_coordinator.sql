CREATE TABLE budget_accounts (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL, data TEXT NOT NULL,
 PRIMARY KEY (tenant_id,id), FOREIGN KEY (tenant_id,id) REFERENCES resource_scopes(tenant_id,id)
);
CREATE TABLE jobs (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL, root_id TEXT NOT NULL, parent_id TEXT,
 budget_id TEXT NOT NULL, session_id TEXT NOT NULL, operation_id TEXT NOT NULL,
 state TEXT NOT NULL, attempt BIGINT NOT NULL, deadline BIGINT NOT NULL,
 cancel_requested BIGINT NOT NULL DEFAULT 0, ready_at BIGINT, wait_reason TEXT,
 spec TEXT NOT NULL, result TEXT, depth BIGINT NOT NULL, retain_until BIGINT NOT NULL,
 PRIMARY KEY (tenant_id,id), UNIQUE (tenant_id,operation_id),
 FOREIGN KEY (tenant_id,id) REFERENCES resource_scopes(tenant_id,id),
 FOREIGN KEY (tenant_id,budget_id) REFERENCES budget_accounts(tenant_id,id)
);
CREATE INDEX job_ready ON jobs(tenant_id,state,ready_at,deadline);
CREATE INDEX job_children ON jobs(tenant_id,root_id,parent_id,state);
CREATE TABLE session_leases (
 tenant_id TEXT NOT NULL, session_id TEXT NOT NULL, job_id TEXT NOT NULL,
 owner_id TEXT NOT NULL, epoch BIGINT NOT NULL, expires_at BIGINT NOT NULL,
 PRIMARY KEY (tenant_id,session_id)
);
CREATE TABLE job_attempts (
 tenant_id TEXT NOT NULL, job_id TEXT NOT NULL, attempt BIGINT NOT NULL,
 owner_id TEXT NOT NULL, epoch BIGINT NOT NULL, started_at BIGINT NOT NULL,
 ended_at BIGINT, outcome TEXT,
 PRIMARY KEY (tenant_id,job_id,attempt), FOREIGN KEY (tenant_id,job_id) REFERENCES jobs(tenant_id,id)
);
CREATE TABLE command_receipts (
 tenant_id TEXT NOT NULL, request_id TEXT NOT NULL, actor_id TEXT NOT NULL,
 resource_id TEXT NOT NULL, kind TEXT NOT NULL, request TEXT, result TEXT,
 PRIMARY KEY (tenant_id,request_id)
);
CREATE TABLE outbox_events (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL, cursor BIGINT NOT NULL,
 resource_id TEXT NOT NULL, kind TEXT NOT NULL, recorded_at BIGINT NOT NULL,
 expires_at BIGINT NOT NULL, PRIMARY KEY (tenant_id,id), UNIQUE (tenant_id,cursor)
);
CREATE INDEX events_cursor ON outbox_events(tenant_id,cursor);
CREATE TABLE event_retention (
 tenant_id TEXT PRIMARY KEY, discarded_through BIGINT NOT NULL
);
CREATE TABLE inbox_receipts (
 tenant_id TEXT NOT NULL, consumer_id TEXT NOT NULL, event_id TEXT NOT NULL,
 job_id TEXT NOT NULL, PRIMARY KEY (tenant_id,consumer_id,event_id)
);
CREATE TABLE budget_reservations (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL, job_id TEXT NOT NULL,
 budget_id TEXT NOT NULL, provider_attempt TEXT NOT NULL, data TEXT NOT NULL,
 PRIMARY KEY (tenant_id,id), UNIQUE(tenant_id,budget_id,provider_attempt),
 FOREIGN KEY (tenant_id,budget_id) REFERENCES budget_accounts(tenant_id,id)
);
CREATE TABLE effect_requests (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL, job_id TEXT NOT NULL,
 logical_operation_id TEXT NOT NULL, kind TEXT NOT NULL, state TEXT NOT NULL,
 epoch BIGINT NOT NULL, request TEXT NOT NULL, receipt TEXT,
 PRIMARY KEY(tenant_id,id), UNIQUE(tenant_id,logical_operation_id,kind),
 FOREIGN KEY(tenant_id,job_id) REFERENCES jobs(tenant_id,id)
);
