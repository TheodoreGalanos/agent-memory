CREATE TABLE entities (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL, provider TEXT NOT NULL, native_id TEXT NOT NULL, label TEXT NOT NULL,
 PRIMARY KEY (tenant_id, id),
 FOREIGN KEY (tenant_id, id) REFERENCES resource_scopes (tenant_id, id)
);
CREATE INDEX entity_identity ON entities (tenant_id, provider, native_id);
CREATE TABLE entity_aliases (
 tenant_id TEXT NOT NULL, entity_id TEXT NOT NULL, alias TEXT NOT NULL,
 PRIMARY KEY (tenant_id, entity_id, alias),
 FOREIGN KEY (tenant_id, entity_id) REFERENCES entities (tenant_id, id)
);
CREATE INDEX alias_lookup ON entity_aliases (tenant_id, alias);
CREATE TABLE entity_links (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL, revision BIGINT NOT NULL,
 from_id TEXT NOT NULL, to_id TEXT NOT NULL, acceptance TEXT NOT NULL, basis TEXT NOT NULL,
 PRIMARY KEY (tenant_id, id),
 FOREIGN KEY (tenant_id, from_id) REFERENCES entities (tenant_id, id),
 FOREIGN KEY (tenant_id, to_id) REFERENCES entities (tenant_id, id)
);
CREATE TABLE artifact_records (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL, revision BIGINT NOT NULL, state TEXT NOT NULL,
 object_key TEXT, attempt_id TEXT, spec TEXT NOT NULL,
 PRIMARY KEY (tenant_id, id),
 FOREIGN KEY (tenant_id, id) REFERENCES resource_scopes (tenant_id, id)
);
CREATE TABLE artifact_dependencies (
 tenant_id TEXT NOT NULL, artifact_id TEXT NOT NULL, depends_on TEXT NOT NULL,
 PRIMARY KEY (tenant_id, artifact_id, depends_on),
 FOREIGN KEY (tenant_id, artifact_id) REFERENCES artifact_records (tenant_id, id),
 FOREIGN KEY (tenant_id, depends_on) REFERENCES artifact_records (tenant_id, id)
);
CREATE TABLE sources (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL, label TEXT NOT NULL, kind TEXT NOT NULL, owner TEXT NOT NULL,
 PRIMARY KEY (tenant_id, id)
);
CREATE TABLE source_versions (
 tenant_id TEXT NOT NULL, source_id TEXT NOT NULL, revision TEXT NOT NULL, scope_id TEXT NOT NULL,
 acquired_at BIGINT NOT NULL, acquisition_method TEXT NOT NULL, precedence TEXT, snapshot_artifact TEXT,
 PRIMARY KEY (tenant_id, source_id, revision),
 FOREIGN KEY (tenant_id, source_id) REFERENCES sources (tenant_id, id),
 FOREIGN KEY (tenant_id, scope_id) REFERENCES resource_scopes (tenant_id, id),
 FOREIGN KEY (tenant_id, snapshot_artifact) REFERENCES artifact_records (tenant_id, id)
);
CREATE TABLE source_locators (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL, source_id TEXT NOT NULL, source_revision TEXT NOT NULL, locator TEXT NOT NULL,
 PRIMARY KEY (tenant_id, id),
 FOREIGN KEY (tenant_id, source_id, source_revision) REFERENCES source_versions (tenant_id, source_id, revision)
);
CREATE TABLE memory_sources (
 tenant_id TEXT NOT NULL, memory_id TEXT NOT NULL, revision BIGINT NOT NULL, locator_id TEXT NOT NULL,
 PRIMARY KEY (tenant_id, memory_id, revision, locator_id),
 FOREIGN KEY (tenant_id, memory_id, revision) REFERENCES memory_versions (tenant_id, id, revision),
 FOREIGN KEY (tenant_id, locator_id) REFERENCES source_locators (tenant_id, id)
);
