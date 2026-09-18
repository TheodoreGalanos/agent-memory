# Memory storage (WP02)

`memory-store` provides the Rust repository used by the later host and process
packages. It uses SQLx for SQLite and PostgreSQL, and object_store for artifact I/O.
Both databases use the same domain types, migrations and repository methods.

## Records and history

`Store::connect` opens a SQLite or PostgreSQL URL and applies migrations in a
transaction. SQLite uses WAL, foreign keys, FULL synchronous writes and a five-second
busy timeout. Migrations preserve existing data and reject a newer schema.

`create_memory`, `create_memories`, `revise_memory` and `apply_memories` retain the
four memory families. `apply_memories` can close an earlier valid interval and
insert a successor atomically. Each batch commits its records, policy decisions
and declared derivation edges together. Arbitrary relation changes are separate
repository calls; the host command transaction boundary arrives in WP03.

Records retain their source locators, actual production inputs, policy revision
and reason. Origin, evidence status, availability and qualification are separate
fields. Procedures can be advisory or reference a published executable artifact.
Intentions retain definitions; occurrence scheduling and completion arrive later.

A correction requires the current revision. It closes the previous transaction
interval and adds a representation under the same memory identity. Changing owning
scope or family requires a new contribution. Policy and relation revisions also
reject stale updates. Old references remain readable under current access rules.

Valid intervals are half-open. An unknown valid time stays unknown and is excluded
when a query specifies `valid_at`. `recorded_as_of` takes a database commit sequence.
The database assigns UTC timestamps; the sequence orders commits even when their
timestamps tie. Current queries return current representations. The default limit
is 100, capped at 1,000; `after_id` pages through memory and relation results.

A single database row serializes domain writes and assigns their sequence. This is
a deliberate experimental implementation: concurrent writers cannot lose updates,
but write throughput has not been qualified. SQLite uses one pooled connection;
PostgreSQL uses up to five. There is no event log or content-hash identity scheme.

## Scope and relationships

Every repository call takes a trusted `Authority`. This library does not authenticate
it; WP03 must obtain it from the authenticated host, never from a worker's claimed
tenant or permissions. An omitted grant dimension is unrestricted within the tenant.

Resource ownership uses user, project, task, entity and source-revision fields.
An unset ownership dimension denotes shared context: a task can read an applicable
project-wide fact. Changing that fact requires a grant covering its complete owning
scope. Personal preferences can have a user scope without a project restriction.

Scope predicates are bound SQL parameters. Entity/source lookups also enforce the
specific assigned identities. Relation endpoints are checked independently, and
artifact reads recheck every retained artifact dependency. A reference or graph
edge never grants permission to read its target. Denied reads return `NotFound`.
Revocation during later reads returns unavailable; already returned bytes cannot
be recalled.

Relations distinguish support, challenge, conflict, derivation, dependency,
succession and triggers. Conflict endpoints are ordered to reject duplicates.
Derivation cycles are rejected; other cycles are allowed. Traversal uses a visited
set and a 10,000-node bound. Shared production inputs retain their identities.
Entity aliases return candidates; an explicit entity link has its own basis,
acceptance and expected revision. Labels alone never merge entities.

## Artifacts

`ArtifactService::local` stores bytes beneath a trusted application directory.
Storage keys stay internal. The sequence is:

1. Allocate a scoped pending record with media type, length, origin and retention class.
2. Stream into a multipart upload under a unique attempt key.
3. Complete the upload; local storage syncs bytes and the containing directory.
4. Publish the ready database state using the expected attempt and revision.

Declared length checks detect incomplete transfers. Pending/uploading artifacts
cannot be read. Reads are limited to 1 MiB; export streams successive bounded
regions. Callers must treat an export error as an incomplete export.

After stopping a failed uploader, `recover_upload` can publish its complete object
or reset a missing upload for another attempt. A late completion from a replaced
attempt cannot publish. Missing stored bytes report `Unavailable`. Revoking an
artifact blocks subsequent reads of it and its dependent artifacts. This is access
revocation, not physical purge.

The object_store boundary accepts other implementations, but only its durable
local filesystem backend has been exercised here. Cloud object storage, grace-period
orphan cleanup and cross-store purge are not qualified. Failed source registration
can leave an unreferenced artifact; retention administration arrives in WP13.

## Source adapters

`SourceService` retains a bounded snapshot at ingestion. A later read uses the
stored revision and locator; it never substitutes the current file. Metadata-only
registrations are allowed and report `Unavailable` when no snapshot is retained.
Source owner, kind and label are stable; revision strings come from the connector.
Source listings are limited to 1,000 visible revisions.

| Adapter | Accepted input | Bounded read |
| --- | --- | --- |
| `DocumentAdapter` | UTF-8 text from bytes or a trusted local file | Inclusive, one-based line range. Optional page is retained locator metadata. |
| `TableAdapter` | CSV with distinct named columns | One-based data row, optionally one column. Quoted fields are parsed with the CSV library. |
| `ModelAdapter` | JSON `entities`, each with a `properties` object | An explicit entity/property pair. No implicit instance-to-type fallback. |
| `ToolEventAdapter` | JSON with `schema_version: "memory-tool-events/1"` and typed `events` | Inclusive, one-based event range, preserving IDs, times, origin and evidence status. |

Tables and models retain normalized JSON snapshots; documents retain text. Both
input and normalized snapshot must fit the configured source limit. Returned JSON
must fit the caller's output limit. Native PDF/BIM extraction, image-region reads
and external trajectory formats need adapters; the current importer does not claim
to understand them. Image locators can be retained as metadata.

## Verification

```sh
cargo test -p memory-store
npm run test:store
npm run check:all
```

The first command runs SQLite tests. PostgreSQL tests are explicitly ignored there;
`test:store` runs the same fixtures on both databases. It initializes a temporary
cluster, listens only on a private Unix socket, and stops/removes the cluster when
the test command finishes. It uses no existing database or credentials.

The shared suite covers corrections, half-open intervals, unknown time, late world
changes, batch rollback, all four families, stale/concurrent writes, scope filters,
relationship direction/cycles, common ancestry, entity aliases/links, source revision
reopening and the C/D property-location example. Artifact tests interrupt uploads,
restart between completion and publication, remove stored bytes, reject late
completion, stream a multi-megabyte export and revoke an upstream dependency.
Separate tests upgrade an earlier schema and reject a newer one.

**Observed on 17 September 2026:** shared repository, migration and coordinator
suites pass on SQLite and PostgreSQL 14.17. An earlier cluster startup failure was
resolved by removing the single orphan shared-memory segment Theo approved, after
checking that it had no attached processes. The test cluster was stopped and
removed after verification. Cloud durability remains unqualified.

Pi session storage has a separate schema and ownership contract. Its PostgreSQL
conformance checks run in the same disposable cluster; see [runtime](runtime.md).
