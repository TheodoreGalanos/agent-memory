CREATE SCHEMA IF NOT EXISTS pi_sessions;
CREATE TABLE IF NOT EXISTS pi_sessions.sessions (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL, metadata JSONB NOT NULL,
 worker_family TEXT NOT NULL, next_seq BIGINT NOT NULL DEFAULT 1, stats JSONB NOT NULL,
 PRIMARY KEY(tenant_id,id)
);
CREATE TABLE IF NOT EXISTS pi_sessions.items (
 tenant_id TEXT NOT NULL, session_id TEXT NOT NULL, id TEXT NOT NULL,
 seq BIGINT NOT NULL, kind TEXT NOT NULL, parent_id TEXT, type TEXT, custom_type TEXT, data JSONB NOT NULL,
 PRIMARY KEY(tenant_id,session_id,id), UNIQUE(tenant_id,session_id,seq),
 FOREIGN KEY(tenant_id,session_id) REFERENCES pi_sessions.sessions(tenant_id,id) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS pi_sessions.values (
 tenant_id TEXT NOT NULL, session_id TEXT NOT NULL, namespace TEXT NOT NULL, key TEXT NOT NULL,
 seq BIGINT NOT NULL, data JSONB NOT NULL,
 PRIMARY KEY(tenant_id,session_id,namespace,key),
 FOREIGN KEY(tenant_id,session_id) REFERENCES pi_sessions.sessions(tenant_id,id) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS pi_sessions.lists (
 tenant_id TEXT NOT NULL, session_id TEXT NOT NULL, namespace TEXT NOT NULL, key TEXT NOT NULL,
 seq BIGINT NOT NULL, data JSONB NOT NULL,
 PRIMARY KEY(tenant_id,session_id,namespace,key,seq),
 FOREIGN KEY(tenant_id,session_id) REFERENCES pi_sessions.sessions(tenant_id,id) ON DELETE CASCADE
);
