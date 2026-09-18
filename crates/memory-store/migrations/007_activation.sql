CREATE TABLE memory_embeddings (
    tenant_id TEXT NOT NULL,
    version_id TEXT NOT NULL,
    data TEXT NOT NULL,
    PRIMARY KEY (tenant_id, version_id)
);
CREATE TABLE search_updates (
    tenant_id TEXT NOT NULL,
    version_id TEXT NOT NULL,
    sequence BIGINT NOT NULL,
    PRIMARY KEY (tenant_id, version_id)
);
CREATE INDEX search_updates_sequence ON search_updates (tenant_id, sequence);
CREATE TABLE activation_windows (
    tenant_id TEXT NOT NULL,
    id TEXT NOT NULL,
    job_id TEXT NOT NULL,
    data TEXT NOT NULL,
    PRIMARY KEY (tenant_id, id)
);

CREATE TABLE search_entities (
    version_id TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    PRIMARY KEY (version_id, entity_id)
);
CREATE INDEX search_entity_lookup ON search_entities (entity_id, version_id);
