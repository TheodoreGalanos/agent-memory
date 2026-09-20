# Agent Memory

An implementation of the [memory architecture](docs/implementation/conceptual-specification.md):
a Rust Host that owns durable storage, jobs, budgets and its own scheduling, and a TypeScript
worker pool that composes process modules — investigation, formation, activation, consolidation,
evaluation (qualification), maintenance and decision — against configured services. See the
[runtime guide](docs/runtime.md) for how it works end to end, and [RELEASE.md](RELEASE.md) for
what is qualified versus experimental, stated honestly and separately for operational
correctness, semantic quality and behavioural value.

The Host serves authenticated commands for jobs, ownership, budgets, receipts and recovery over
HTTP; a worker pool child claims eligible work, drives it with an assigned Pi worker, and settles
it, with lease-based recovery on crash. The Host also schedules work on its own timer: a settled
task's own published experience triggers formation; a cohort large enough triggers consolidation;
a retained candidate procedure triggers qualification; a correction or source change triggers
maintenance. Every scheduling decision is recorded, so a disabled module's triggers accumulate as
scheduled-but-unclaimed work rather than being lost. `packages/supervisor/test/learning.test.ts`
proves the resulting loop — formation, cohort consolidation, qualification, adoption, later
retrieval, a correction that schedules maintenance, an intention firing — genuinely unattended.

The Harbor bridge provisions and recovers Docker sandbox allocations, renews endpoint ownership,
publishes exports and verifies teardown for native tool execution; ASP provides the same over
SSH. Formation reads structured captured experience and, question by question, judges what is
worth keeping. Activation retrieves by identity, lexical search and (with local Nomic embeddings)
vector similarity, then checks applicability before anything enters a task's context.
Consolidation groups related memories, proposes a method, and only adopts it once qualification
compares it against held-out cases it never trained on. Maintenance revises or retires guidance
when what it depended on changes, preserving history. Intentions are durable commitments checked
against real completion evidence, never silence. A task can attach a decision — a bounded menu of
next steps the Host rechecks before anything acts on it.

**Judgement.** Every question in the 31-family catalogue runs through a configured route
(`primary`, with optional `fallback` and `comparison`) — Jev or a conventional generative model,
never a mock. The runtime's own evaluation status is `qualified` only when an operator supplies a
held-out gate covering a family, definition revision, model release and scope; **no gate is
configured anywhere in this repository**, so every family is honestly `experimental` today (see
[RELEASE.md](RELEASE.md)).

## Try the system

[Follow the step-by-step user guide](docs/user-guide.md) to start a local Host and use the
API/CLI for inspection, corrections, teaching, temporary exploration, commitments, decisions and
notifications. It needs no model credentials or paid services. To see judgement, formation and the
Host's own scheduling work unattended against a real worker pool, see
[Running it](docs/runtime.md#running-it) in the runtime guide — those tests are also credential-free.

## Run the checks

Requirements: Rust/Cargo, Node >=22.19, npm, and `tar`. The lockfiles record the resolved
dependencies.

```sh
npm run setup
npm run check
```

Setup downloads the pinned Pi source, installs the locked npm packages, and runs Pi's public
model-catalogue downloader. It requires network access. Pi's catalogue is generated public
metadata; the tests select only its pinned `faux/faux-1` provider. Python 3 is also needed for
the local interpreter fixture. No API keys, paid model calls, Docker, or remote services are
needed for the default checks. HTTP tests use task-owned loopback listeners. The first Rust build
also downloads the locked Cargo dependencies; subsequent builds and tests can run offline.
PostgreSQL parity checks also need Python 3 and PostgreSQL development binaries (`pg_config`,
`initdb`, `pg_ctl`).

Useful shorter commands:

```sh
npm run build          # Build the host and Pi, generate contracts, type-check the worker
npm test               # Rust contracts/store/HTTP, Pi worker and Python bridge tests
npm run test:upstream  # Pi in-memory conformance and SQLite backend suite
npm run test:store     # Both domain backends and PostgreSQL Pi/workspace checks
npm run check:all      # Full project checks plus PostgreSQL parity
npm run test:asp       # Real loopback OpenSSH; requires sshd and ssh-keygen
npm run test:docker    # Actual Harbor sandbox, Pi tools, export and teardown
npm run inventory      # Capability inventory: every pool module and judgement family
```

Scripted and faux providers (`MEMORY_TEST_SCRIPTED=1`, plain Faux, `faux/faux-1`) exist only in
tests; they verify the harness and contracts, not model reasoning quality. Live model behaviour
remains unqualified; sandbox qualification covers only the Docker and bounded Daytona profiles.

## Where to work

- [Runtime guide](docs/runtime.md): composition, the schedulers, judgement, formation, activation,
  consolidation/qualification, maintenance, the decision attachment, and how to run the
  composition matrix and the headless continuous scenario.
- [Release status](RELEASE.md): what is qualified versus experimental.
- [Rust contracts](crates/memory-domain/src/contracts.rs) and [generated wire types](contracts/README.md)
- [Memory repositories and storage contract](docs/implementation/storage.md)
- [Runbooks](docs/runbooks.md) and [production manifests](deploy/README.md)
- [Harbor and ASP boundaries](services/harbor-bridge/README.md)
- [Campaign scaffold](evals/campaign/README.md) and [property-location fixtures](evals/property-location/README.md)
