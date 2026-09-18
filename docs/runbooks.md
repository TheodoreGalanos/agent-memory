# Runbooks (§20.5)

Each runbook names how the condition shows up, what to do with the existing commands, and
which automated test exercises it. "Exercised" means a test in this repository drives the
condition against a real Host; "documented" means the procedure exists but no test drives it yet.
Commands assume the CLI (`npm run memory -- …`) and an administrator credential unless stated.

## Detection surface

- `GET /healthz` — process alive. `GET /readyz` — database reachable. `GET /metrics` — job counts
  by state, issued worker credentials, uptime (Prometheus text, no identifiers).
- Host terminal — pool trace (`pool N: claimed…`, judgement `→/←` lines, `failed:`, `blocked:`).
- `changes --config CLIENT.json` — scoped event stream (`job_recovered`, `cancellation_requested`, …).
- `budget_usage` — committed spend and unresolved reservations per budget.

## 1. Worker crash — *exercised*

**Symptom.** `metrics` shows `leased`/`running` jobs that do not progress; the Host terminal shows
a pool child exit and restart (`Worker pool N: child exited…`).
**Procedure.** Nothing manual. The lease expires; the Host runs coordinator `recover` every five
seconds and returns the job to `queued` with its attempt count intact; the restarted pool claims it.
After `max_attempts` the job is `failed`; inspect with `task --id` and resubmit a revised brief.
**Test.** `packages/supervisor/test/supervisor.test.ts` — "a worker that dies after claiming";
`crates/memory-host/tests/supervisor.rs` — child restart and termination.

## 2. Provider outage — *exercised*

**Symptom.** Pool trace shows `← reference … failed:` lines; formation results are `partial` with
deferred candidates; task jobs settle `partial` with `provider_failed` manifests.
**Procedure.** Judgement records the failure as an `unavailable` assessment and defers the
candidate; nothing is invented. When the provider returns, submit a new formation job over the same
source with a **new purpose** (the purpose is the formation operation; the same purpose would
continue the source cursor past the deferred events). For dispatch-level outages, disable the
provider with `set_control {kind: provider}` so new admissions wait, and re-enable afterwards.
**Test.** `supervisor.test.ts` — "a judgement provider outage defers the candidates…".

## 3. Sandbox disconnect — *exercised in WP05/WP07*

**Symptom.** Python/ASP tool calls fail; effect requests show `outcome_unknown`.
**Procedure.** The Harbor bridge reconciles orphans and restarts on its own timer; unknown effects
block `complete` until `reconcile_effect` records the real outcome. See
`services/harbor-bridge/README.md`.
**Test.** `npm run test:docker` (real Docker), `crates/memory-host/tests/sandbox_lifecycle.rs`.

## 4. Source revocation — *exercised in WP12/WP13*

**Symptom.** `changes` shows `memory_maintained`/revocation events; workers report `blocked`
results naming the revised input revision.
**Procedure.** Corrections and revocations publish change notices; unfinished dependent work is
marked for recheck. A pool worker whose brief grants a superseded memory revision settles the job
`blocked` with the reason — resubmit with the current reference. Deletion follows the WP13 flow
(`begin_deletion`, acknowledgements, `purge_deletion`).
**Test.** `supervisor.test.ts` (stale input → `blocked`), `crates/memory-store/tests/retention.rs`,
`crates/memory-host/tests/retention.rs`.

## 5. Stuck intention — *exercised in WP12*

**Symptom.** `inspect_intentions` shows an occurrence `pending` past its expected trigger.
**Procedure.** Confirm readiness inputs exist (plan, ready artifacts/memories); the Host timer
sweeps intentions every second. Use `cancel_intention` or `confirm_intention` explicitly; never
treat silence as completion.
**Test.** `crates/memory-store/tests/maintenance.rs`, `crates/memory-host/tests/maintenance.rs`.

## 6. Unknown effect — *exercised in WP03/WP13*

**Symptom.** `inspect_effect` shows `outcome_unknown`; `complete` is refused with a conflict.
**Procedure.** Query the effect's owner or receipt; record the truth with `reconcile_effect`
(or `reconcile_deleted_effect` during clean continuation). Only then can the job complete.
**Test.** `crates/memory-host/tests/result_publication.rs`, `retention.rs`.

## 7. Database failover — *documented*

**Symptom.** `/readyz` returns 503 "database unreachable"; pool trace shows Host command failures.
**Procedure (PostgreSQL).** Promote the replica, repoint `database_url`, restart the Host. Leases
and reservations are durable; recovery requeues expired leases; unknown effects are reconciled per
runbook 6. Worker sessions use the fenced PostgreSQL backend, so a superseded epoch cannot write.
**Procedure (SQLite).** Restore from the latest backup (runbook 11) if the file is unusable.
**Test.** None drives a live failover; fencing is tested in `packages/pi-worker/test/postgres.test.ts`.

## 8. Index corruption — *documented*

**Symptom.** Text or vector search returns nothing for known records; `memory_search` queries error.
**Procedure.** Search tables are projections. For SQLite, rebuild with `INSERT INTO
memory_search(memory_search) VALUES('rebuild')` while the Host is stopped; for PostgreSQL, truncate
`memory_search` and re-run the version-7 backfill (`crates/memory-store/src/database.rs`, migration
7 path) or re-index embeddings with `npm run embeddings -- index`. Domain correctness never depends
on the projection.
**Test.** Embedding index consistency in `packages/activation/test`; no corruption drill.

## 9. Budget mismatch — *exercised*

**Symptom.** `budget_usage` shows large `unresolved` reservations; jobs fail with
`budget_exhausted` at `reserve`.
**Procedure.** Reservations without cost telemetry hold their maximum until the job settles (an
unknown bill is never treated as zero). Size job `max_tokens` for providers × candidates ×
reservation, or lower the judgement maximum in `worker.json`. A cancelled job's reservations are
released by recovery. Compare `committed` with the provider invoice at the end of the period.
**Test.** Reservation/settlement paths in `crates/memory-host/tests/judgement.rs`; the live
walkthrough documents the ledger before and after formation.

## 10. Failed purge — *exercised in WP13*

**Symptom.** `inspect_deletion` reports obligations pending (provider retention, backups, storage).
**Procedure.** Acknowledge each external obligation only with evidence (`acknowledge_purge`);
`purge_deletion` completes live removal. Restores must carry the deletion registry (runbook 11).
**Test.** `crates/memory-store/tests/retention.rs`, `crates/memory-host/tests/retention.rs`.

## 11. Backup, restore and rollback — *exercised (local profile)*

**Procedure.** `backup DIR --config ADMIN.json` writes a consistent SQLite snapshot, an artifact
copy, `deletions.json` and `manifest.json` (recovery position, schema version, checksums).
`restore BACKUP_DIR NEW_DIR --host ORIGINAL/host.json [--registry CURRENT_DELETIONS.json]` restores
into isolation: checksum verified, `restore_registry` applied by the Host before it serves any
request, worker pool disabled until reviewed. Rollback after a failed upgrade or migration is the
same restore into a fresh directory, then redirecting clients; never start the old copy alongside.
For PostgreSQL use base backup + WAL archiving and object-storage versioning; keep
`list_deletions` output with each backup.
**Test.** `crates/memory-host/tests/backup.rs`, `packages/cli/test/backup-restore.test.ts` (restore
applies a deletion recorded after the backup).

## 12. Local-to-production migration — *exercised against a disposable cluster*

**Procedure.** Stop the local Host (drains writers). `backup` for a consistent snapshot. Create an
empty PostgreSQL database and run `cargo run --bin memory-migrate -- sqlite://SNAPSHOT.db?mode=rw
postgresql://…`: every table is copied in foreign-key order under unchanged identities, the commit
clock is carried across, counts are verified, and a non-empty target is refused. Then point a
production `host.json` at the target, start the Host, compare `browse`/`history` on both, and only
then redirect clients. Pi sessions in local SQLite files are not migrated; finish or cancel their
jobs before migrating (clean continuation, WP13).
**Test.** `crates/memory-store/tests/migrate.rs` (runs under `npm run test:store`).

## 13. Upgrades and canaries — *documented*

Pin model, question and profile identity in reports (`judgement_usage`, `RenderManifest.profile`).
Before a wider rollout, run the fixture suites (`npm run check`, `npm run test:store`) and the
Part 1 walkthrough on the new build; then let one pool child with a narrow `classes` grant claim
selected work while the old build keeps the rest. Sessions incompatible with a worker upgrade are
drained by cancelling their jobs and resubmitting.
