# Production profile

These manifests implement §20.2 of the specification for one Host with its worker pool,
PostgreSQL with separate domain and session roles, file-based secrets and a TLS reverse
proxy. They are written and syntax-checked (`docker compose -f deploy/compose.yaml config`);
a full image build and a live production run have **not** been performed in this repository.

## Layout

- `Dockerfile` — Rust release build of `memory-host`, then a Node 22 runtime with the pinned
  Pi packages for the worker pool. Runs as an unprivileged user under `tini`.
- `compose.yaml` — `postgres` (roles created by `postgres-init.sql`), `host` (binds
  loopback inside the container; only the proxy reaches it), `proxy` (Caddy TLS).
- `host.json` / `worker.json` — mounted read-only. Use `database_url` with the
  `memory_domain` role; the pool's `worker.json` uses the `memory_session` role for the
  PostgreSQL Pi session backend. Tokens for client, administrator and pool credentials are
  generated per deployment (`npm run memory -- init` produces a template you can adapt).
- `secrets/` — one file per secret; never commit this directory.

## Bring-up

1. Create `deploy/secrets/*` and `deploy/host.json`, `deploy/worker.json`, `deploy/Caddyfile`.
2. `npm run setup` on the build machine so `.upstream/` exists for the image build.
3. `docker compose -f deploy/compose.yaml up -d --build`.
4. `curl -k https://memory.example.internal/readyz` → `ready: database reachable`.
5. Scrape `/metrics` (job counts by state, issued credentials, uptime) from the proxy network.

## Capacity

Interactive and deferred capacity are separate pool slots (`worker.json` `concurrency`).
Add pool credentials with narrower `classes` and scale `host` replicas per class; every pool
child claims through `claim_next`, which serves projects with fewer active jobs first.

## Backups

The SQLite `backup` command does not apply here. Use PostgreSQL base backups with WAL
archiving for both schemas, object-storage versioning for artifacts, and keep the deletion
registry (`list_deletions`) with each backup; restore follows the same isolation-first
procedure as the local profile (see the runbooks).
