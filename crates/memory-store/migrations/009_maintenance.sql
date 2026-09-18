CREATE TABLE intention_occurrences (
 tenant_id TEXT NOT NULL,id TEXT NOT NULL,definition_id TEXT NOT NULL,revision BIGINT NOT NULL,
 cycle BIGINT NOT NULL,state TEXT NOT NULL,due_at BIGINT,expires_at BIGINT,data TEXT NOT NULL,polled_at BIGINT NOT NULL DEFAULT 0,recurring BIGINT NOT NULL DEFAULT 0,
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,definition_id,revision,cycle)
);
CREATE INDEX intention_due ON intention_occurrences(tenant_id,state,due_at,expires_at);
CREATE TABLE intention_checks (
 tenant_id TEXT NOT NULL,id TEXT NOT NULL,job_id TEXT NOT NULL,data TEXT NOT NULL,
 PRIMARY KEY(tenant_id,id)
);
CREATE TABLE maintenance_reviews (
 tenant_id TEXT NOT NULL,id TEXT NOT NULL,job_id TEXT NOT NULL,data TEXT NOT NULL,result TEXT,
 PRIMARY KEY(tenant_id,id)
);
CREATE TABLE memory_changes (
 tenant_id TEXT NOT NULL,id TEXT NOT NULL,cursor BIGINT NOT NULL,resource_id TEXT NOT NULL,data TEXT NOT NULL,
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,cursor)
);
