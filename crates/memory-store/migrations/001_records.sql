CREATE TABLE resource_scopes (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL, user_id TEXT, project_id TEXT, task_id TEXT,
 PRIMARY KEY (tenant_id, id)
);
CREATE INDEX scope_projects ON resource_scopes (tenant_id, project_id, user_id, task_id);
CREATE TABLE scope_entities (
 tenant_id TEXT NOT NULL, scope_id TEXT NOT NULL, entity_id TEXT NOT NULL,
 PRIMARY KEY (tenant_id, scope_id, entity_id),
 FOREIGN KEY (tenant_id, scope_id) REFERENCES resource_scopes (tenant_id, id)
);
CREATE INDEX scope_entity_lookup ON scope_entities (tenant_id, entity_id, scope_id);
CREATE TABLE scope_sources (
 tenant_id TEXT NOT NULL, scope_id TEXT NOT NULL, source_id TEXT NOT NULL, revision TEXT NOT NULL,
 PRIMARY KEY (tenant_id, scope_id, source_id, revision),
 FOREIGN KEY (tenant_id, scope_id) REFERENCES resource_scopes (tenant_id, id)
);
CREATE INDEX scope_source_lookup ON scope_sources (tenant_id, source_id, revision, scope_id);
CREATE TABLE policies (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL, label TEXT NOT NULL, revision BIGINT NOT NULL,
 PRIMARY KEY (tenant_id, id),
 FOREIGN KEY (tenant_id, id) REFERENCES resource_scopes (tenant_id, id)
);
CREATE TABLE policy_versions (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL, revision BIGINT NOT NULL, data TEXT NOT NULL,
 PRIMARY KEY (tenant_id, id, revision),
 FOREIGN KEY (tenant_id, id) REFERENCES policies (tenant_id, id)
);
CREATE TABLE policy_decisions (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL, policy_id TEXT NOT NULL, policy_revision BIGINT NOT NULL,
 actor_id TEXT NOT NULL, sequence BIGINT NOT NULL, data TEXT NOT NULL,
 PRIMARY KEY (tenant_id, id),
 FOREIGN KEY (tenant_id, policy_id, policy_revision) REFERENCES policy_versions (tenant_id, id, revision)
);
CREATE TABLE memory_records (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL, family TEXT NOT NULL, revision BIGINT NOT NULL,
 created_by TEXT NOT NULL,
 PRIMARY KEY (tenant_id, id),
 FOREIGN KEY (tenant_id, id) REFERENCES resource_scopes (tenant_id, id)
);
CREATE TABLE memory_versions (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL, revision BIGINT NOT NULL, version_id TEXT NOT NULL,
 recorded_from BIGINT NOT NULL, recorded_to BIGINT, recorded_at BIGINT NOT NULL,
 valid_known BIGINT NOT NULL, valid_from BIGINT, valid_to BIGINT,
 availability TEXT NOT NULL, decision_id TEXT NOT NULL, data TEXT NOT NULL,
 PRIMARY KEY (tenant_id, id, revision), UNIQUE (tenant_id, version_id),
 FOREIGN KEY (tenant_id, id) REFERENCES memory_records (tenant_id, id),
 FOREIGN KEY (tenant_id, decision_id) REFERENCES policy_decisions (tenant_id, id),
 CHECK (valid_to IS NULL OR valid_from IS NULL OR valid_from < valid_to)
);
CREATE INDEX memory_time ON memory_versions (tenant_id, recorded_from, recorded_to, valid_from, valid_to);
CREATE TABLE relation_records (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL, revision BIGINT NOT NULL, kind TEXT NOT NULL,
 from_id TEXT NOT NULL, to_id TEXT NOT NULL, from_revision BIGINT NOT NULL, to_revision BIGINT NOT NULL,
 PRIMARY KEY (tenant_id, id),
 UNIQUE (tenant_id, kind, from_id, from_revision, to_id, to_revision),
 FOREIGN KEY (tenant_id, id) REFERENCES resource_scopes (tenant_id, id),
 FOREIGN KEY (tenant_id, from_id, from_revision) REFERENCES memory_versions (tenant_id, id, revision),
 FOREIGN KEY (tenant_id, to_id, to_revision) REFERENCES memory_versions (tenant_id, id, revision)
);
CREATE INDEX relations_outgoing ON relation_records (tenant_id, from_id, kind);
CREATE INDEX relations_incoming ON relation_records (tenant_id, to_id, kind);
CREATE TABLE relation_versions (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL, revision BIGINT NOT NULL, version_id TEXT NOT NULL,
 recorded_from BIGINT NOT NULL, recorded_to BIGINT, recorded_at BIGINT NOT NULL,
 valid_known BIGINT NOT NULL, valid_from BIGINT, valid_to BIGINT, acceptance TEXT NOT NULL, data TEXT NOT NULL,
 PRIMARY KEY (tenant_id, id, revision),
 FOREIGN KEY (tenant_id, id) REFERENCES relation_records (tenant_id, id)
);
