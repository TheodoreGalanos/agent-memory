CREATE TABLE explorations (
 tenant_id TEXT NOT NULL,id TEXT NOT NULL,revision BIGINT NOT NULL,data TEXT NOT NULL,
 PRIMARY KEY(tenant_id,id)
);
CREATE TABLE decision_requests (
 tenant_id TEXT NOT NULL,id TEXT NOT NULL,job_id TEXT NOT NULL,revision BIGINT NOT NULL,answer TEXT,data TEXT NOT NULL,
 PRIMARY KEY(tenant_id,id)
);
CREATE TABLE task_contexts (
 tenant_id TEXT NOT NULL,id TEXT NOT NULL,job_id TEXT NOT NULL,data TEXT NOT NULL,
 PRIMARY KEY(tenant_id,id)
);
CREATE TABLE notification_preferences (
 tenant_id TEXT NOT NULL,actor_id TEXT NOT NULL,mode TEXT NOT NULL,
 PRIMARY KEY(tenant_id,actor_id)
);
CREATE TABLE notification_deliveries (
 tenant_id TEXT NOT NULL,actor_id TEXT NOT NULL,event_id TEXT NOT NULL,
 PRIMARY KEY(tenant_id,actor_id,event_id)
);
CREATE TABLE runtime_controls (
 tenant_id TEXT NOT NULL,kind TEXT NOT NULL,target TEXT NOT NULL,revision BIGINT NOT NULL,enabled BIGINT NOT NULL,
 PRIMARY KEY(tenant_id,kind,target)
);
