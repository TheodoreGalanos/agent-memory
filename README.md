# Agent Memory

Experimental implementation of the [memory architecture](docs/implementation/conceptual-specification.md).
The [implementation plan](docs/implementation/plan.md) describes all sixteen work packages.

**Current increment: WP14 (API and CLI; web UI deferred).** The Rust host now serves authenticated commands
for jobs, ownership, budgets, receipts and recovery. Assigned Pi workers persist
and resume work with local SQLite or fenced PostgreSQL sessions. Result publication
stores an artifact before committing application completion.

The Harbor bridge now provisions and recovers Docker allocations, renews endpoint
ownership, publishes sandbox exports and verifies teardown. The complete local
path passes with the real Rust host, pinned Harbor and Pi native tools over SSH.
The same path passed on the bounded Daytona test profile. See the
[plan](docs/implementation/plan.md) and [bridge guide](services/harbor-bridge/README.md).

WP06 adds typed workspace checkpoints, source inventories, conflict-aware context
selection and recorded provider context. Required state survives Pi compaction and
restore. Access changes rebuild context; budget failures stop the provider call.
See the [workspace runtime](docs/implementation/runtime.md#workspace-state-and-context).

WP08 adds the 29-family semantic judgement catalogue, evidence-backed packets,
Jev and conventional-model routes, parallel question batches, policy decisions,
scoped reuse and task-local checks. See the
[judgement runtime](docs/implementation/runtime.md#semantic-judgement-wp08).
Jev defaults to shadow mode. The small live smoke test verified the API and batching
but found semantic disagreements; no family is qualified for automatic use.

WP09 adds incremental formation and inspected record references. Its live diagnostic
found semantic failures in the Azure reference; those remain open. WP10 adds scoped
entity, text and exact-vector retrieval, method applicability checks and grouped
context delivery to the workspace. SQLite/PostgreSQL and scripted Host/Pi flows are
verified. Local Nomic embeddings now support indexing and query generation;
two real-model paraphrase smoke cases pass. Broader retrieval usefulness and model
quality remain unqualified.
See the [activation runtime](docs/implementation/runtime.md#activation-and-search-wp10).

WP11 adds bounded consolidation and procedure qualification. It groups shared-source
accounts, preserves exceptions and conditions, and retains proposed methods as
candidates. Evaluation child jobs compare original episodes, a concise summary and
the proposed method. The Host adopts successful methods under configured rules and
keeps failed methods as candidates. Advisory and executable paths have local
functional coverage. A small paid run exercised live synthesis and transfer, but
its Azure judge accepted an unsupported generalisation. Semantic retention remains
unqualified. See the [live report](evals/consolidation/live-2026-09-18.json).
See the [consolidation runtime](docs/implementation/runtime.md#consolidation-and-qualification-wp11).

WP12 adds maintenance and intention execution. Corrections preserve temporal history;
source loss retires dependent guidance while retaining independently supported claims.
Unfinished workspace conclusions are selectively marked for rechecking. Intention
occurrences have durable triggers, atomic job creation, checked completion,
confirmation, cancellation, expiry and fixed-interval recurrence. See the
[maintenance runtime](docs/implementation/runtime.md#maintenance-and-intentions-wp12).

WP13 adds revocation before cleanup, tracked worker exposure, deletion of governed
copies and clean-session continuation. Reports keep live removal separate from
provider, backup and database storage obligations. Local and PostgreSQL session
administration reject restored deleted identities. See the
[retention runtime](docs/implementation/runtime.md#retention-erasure-and-administration-wp13).

WP14 adds scoped user and operator operations through the same authenticated
command endpoint and a thin CLI: browsing and history, task context inspection
from recorded render manifests, corrections that publish workspace change notices,
attributed teaching and preference records, temporary explorations with explicit
promotion, prepared owner decisions that block only the affected job, pull
notifications with actor preferences, and administrator policy/dispatch controls.
See the [user and operator API/CLI](docs/implementation/runtime.md#user-and-operator-apicli-wp14).


**Release candidate:** [RELEASE.md](RELEASE.md) freezes the version matrix, lists every
judgement family's mode (all shadow) and fallback, and separates operational, semantic and
behavioural results. Semantic qualification and the §21 stories as continuous runs are open.

## Try the system

[Follow the step-by-step user guide](docs/user-guide.md) to start a local Host and
use the API/CLI for inspection, corrections, teaching, temporary exploration,
commitments, decisions and notifications. Part 1 uses SQLite and needs no model
credentials or paid services. Part 2 (`npm run live`) continues on the same instance
with a live model working a task, formation judging its findings with Azure as
reference and Jev in shadow, and the CLI showing what memory retained; it costs about
a cent per run. WP14 delivers these API and CLI surfaces; web UI is deferred. WP15 adds
the local worker pool (`npm run memory -- enable-pool DIRECTORY`): the Host spawns a Node
pool child that claims queued investigation and formation jobs fairly with runtime
job-bound credentials and settles them unattended, plus recovery, backup/restore,
health/metrics, deployment manifests, SQLite→PostgreSQL migration and
[runbooks](docs/runbooks.md).

## Run the checks

Requirements: Rust/Cargo (tested with 1.96.0), Node >=22.19 (tested with 26.4.0), npm
(tested with 11.17.0), and `tar`. The lockfiles record the resolved dependencies.

```sh
npm run setup
npm run check
```

Setup downloads the pinned Pi source, installs the locked npm packages, and runs
Pi's public model-catalogue downloader. It requires network access. Install scripts
are disabled: Pi's optional image-test dependency is unnecessary for this increment.
Pi's catalogue is generated public metadata; the tests select only its pinned
`faux/faux-1` provider. Python 3 is also needed for the local interpreter fixture. No API keys, paid model calls, Docker, or remote services are
needed for the default checks. HTTP tests use task-owned loopback listeners. The first Rust build also downloads the locked Cargo dependencies.
Subsequent builds and tests can run offline. PostgreSQL parity checks also need
Python 3 and PostgreSQL development binaries (`pg_config`, `initdb`, `pg_ctl`).

Useful shorter commands:

```sh
npm run build          # Build the host and Pi, generate contracts, type-check the worker
npm test              # Rust contracts/store/HTTP, Pi worker and Python bridge tests
npm run test:upstream  # Pi in-memory conformance and SQLite backend suite
npm run test:store     # Both domain backends and PostgreSQL Pi/workspace checks
npm run check:all      # Full project checks plus PostgreSQL parity
npm run test:asp       # Real loopback OpenSSH; requires sshd and ssh-keygen
npm run test:docker    # Actual Harbor sandbox, Pi tools, export and teardown
```

The scripted provider exists only in tests. It verifies the harness and contracts;
it supplies no evidence about model reasoning quality. Live model behavior remains unqualified; sandbox qualification covers only the
Docker and bounded Daytona profiles recorded in the plan.

## Where to work

- [Implementation plan and progress](docs/implementation/plan.md)
- [Upstream compatibility and prerequisites](docs/implementation/compatibility.md)
- [Requirements and implementation locations](docs/implementation/traceability.md)
- [Rust contracts](crates/memory-domain/src/contracts.rs) and [generated wire types](contracts/README.md)
- [Memory repositories and storage contract](docs/implementation/storage.md)
- [Coordinator, worker and session operation](docs/implementation/runtime.md)
- [Runbooks](docs/runbooks.md) and [production manifests](deploy/README.md)
- [Harbor and ASP boundaries](services/harbor-bridge/README.md)
- [Property-location fixtures](evals/property-location/README.md)
