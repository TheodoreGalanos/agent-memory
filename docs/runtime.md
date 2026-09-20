# Runtime guide

This describes the memory system as it runs today: a Rust Host that owns durable
storage, jobs, budgets and scheduling, and a TypeScript worker pool that composes
process modules against configured services and claims work from it. Nothing here
describes a plan or a history of changes; it describes the current behaviour.

## The loop, in plain English

Things happen — a task does work, a source is captured, a person corrects a
statement. Some of what happened is worth remembering, so **formation** looks at
the recorded experience and, question by question, decides what to keep as a
candidate: what kind of thing it is, whether it is faithfully described, whether
the evidence actually supports it. A later task's own investigation is offered
that memory through **activation**: retrieval finds candidates, judgement checks
whether they actually apply here, and only what is both found and checked enters
the task's context.

Some kept candidates are worth turning into general knowledge. **Consolidation**
groups accounts that share a source, proposes a common method, and asks judgement
whether the proposal is honestly bounded and whether it holds on cases it never
saw — that is **qualification**. Only a method that passes qualification is
adopted for others to use; a method that fails stays a candidate, not silently
discarded and not silently promoted. Things also change: a correction or a lost
source can undermine what was kept, so **maintenance** rechecks dependent
guidance and revises or retires it, preserving history rather than erasing it. An
**intention** is a durable commitment — "revisit this once X happens" — that
fires on its own trigger and is checked, not assumed, complete. A task can also
attach a **decision**: a bounded menu of next steps built from what it actually
observed, with judgement's pick rechecked by the Host before anything acts on it.

None of this needs a human to drive it by hand. The Host itself watches for the
triggers (a task's own published experience, a cohort large enough to
consolidate, a retained procedure ready to qualify, a correction or source
change) and schedules the corresponding job. See
[Schedulers](#schedulers-the-hosts-own-triggers) below for exactly which triggers
schedule what, and
[`packages/supervisor/test/continuous.test.ts`](../packages/supervisor/test/continuous.test.ts)
for this loop running unattended against the real entrypoint.

## Composition: services, modules, worker.json

A worker pool child is one OS process (`packages/supervisor/src/main.ts`) that
reads a `worker.json`, installs only the services its enabled processes need, and
composes the process modules that can run against those services — all before its
pool credential claims anything.

**Services** (`Services` in `packages/supervisor/src/composition.ts`) are the
capabilities a module can require or use optionally:

- `taskModel` — a Pi model plus payload/reservation limits, for processes that
  drive an assigned Pi worker (investigation) or run judged synthesis
  (consolidation, evaluation).
- `judgement` — `{ primary, fallback?, comparison? }` judgement providers plus a
  shared per-attempt maximum, for every process that asks the catalogue a
  question (formation, consolidation, maintenance, activation, decision).
- `execution` — where investigation's and evaluation's granted native tools
  (`read`, `write`, `edit`, `bash`, `python`) actually run: a `node` working
  directory or an `asp` (Harbor/SSH) binding. Its absence means investigation
  tasks are tool-less, not that they fail.

**`worker.json`** declares which processes are enabled and how each service is
configured:

```json
{
  "sessions_directory": "pool",
  "processes": ["investigation", "formation", "activation"],
  "task_model": { "provider": "azure-openai-responses", "model": "gpt-5.4-mini" },
  "judgement": {
    "primary": { "kind": "jev", "model": "jev-latest" },
    "fallback": { "kind": "generative", "provider": "azure-openai-responses", "model": "gpt-4.1-mini" }
  },
  "execution": { "kind": "node", "cwd": "/path/to/a/working/directory" },
  "concurrency": { "interactive": 1, "deferred": 1 },
  "poll_ms": 1000,
  "lease_seconds": 120,
  "drain_ms": 5000
}
```

The `judgement` block names up to three routes, not "shadow" or "reference"
modes: **`primary`** is the operational route that decides every assessment;
**`fallback`** (optional) is consulted only when the primary yields no usable
answer; **`comparison`** (optional) is a separately budgeted route whose answer
is recorded *after* the decision, for comparison, and never changes it. Each
route is `{"kind":"jev", model}` or `{"kind":"generative", provider, model}` — a
real Pi model, no mock mode. `npm run memory -- enable-pool DIRECTORY` writes a
starting `worker.json` and adds a `pool` credential and `worker_pool` launch
configuration to `host.json`; see [runbooks](runbooks.md) and the
[user guide](user-guide.md).

**Pool modules** (`POOL_MODULES` in `packages/supervisor/src/index.ts`) are the
seven process handlers: `investigation`, `formation`, `consolidation`,
`evaluation`, `maintenance`, `activation`, `decision`. Each declares an id,
version, required and optional services, the job processes it handles
(`operations`), and any application tools it offers. `composeApplication`
(`packages/supervisor/src/composition.ts`) rejects the composition — before any
job is claimed — if a required service is missing, if two modules claim the same
process, or if a tool collides with another module's tool or a native execution
tool name. A module that declares no `operations` (only `decision`, which offers
the `decide` tool) joins whenever its required services are installed, regardless
of `processes`. The resulting composition identity
(`investigation@3+formation@1+activation@1+decision@1#<digest>`) is sent with
`start` and recorded on the job (`Job.composition`), so inspection shows which
composition actually accepted the work. Provider swaps within a service (e.g.
one generative provider for another) never affect this decision; only a missing
*service* does. `packages/supervisor/test/compositions.test.ts` runs a small job
through several compositions — investigation alone, investigation with judgement
but no memory processes, read-only memory (activation without formation),
capture-only (formation without consolidation/maintenance), full learning, and
full learning minus one module — asserting each composes or is rejected as
described, that a disabled module leaves existing data and its `scheduled_work`
entries intact rather than lost, and that shutdown drains or cancels outstanding
work within `drain_ms` rather than abandoning it.

`npm run inventory` (`scripts/inventory.mjs`) prints every pool module's
declaration and every judgement catalogue family's ownership, permitted uses and
eval-fixture coverage, generated directly from `POOL_MODULES` and the catalogue
so it can never drift from what actually runs:

## Pool modules (7)

| id | version | requires | optional | operations | tools offered |
| --- | --- | --- | --- | --- | --- |
| investigation | 3 | taskModel | judgement, execution, adapters | investigation | — |
| formation | 1 | judgement | — | formation | — |
| consolidation | 1 | judgement | taskModel | consolidation | — |
| evaluation | 1 | — | execution, taskModel | evaluation | — |
| maintenance | 1 | judgement | — | maintenance | — |
| activation | 1 | judgement | — | activation | — |
| decision | 1 | judgement | — | — | decide |

## Judgement catalogue families (31)

| id | owner | permitted uses | eval fixture |
| --- | --- | --- | --- |
| J01 | Formation | candidate_formation | yes |
| J02 | Formation/shared | candidate_formation_shared | yes |
| J03 | Formation | candidate_formation | yes |
| J04 | Formation | candidate_formation | yes |
| J05 | Formation/intentions | candidate_formation_intentions | yes |
| J06 | Activation | candidate_activation | yes |
| J07 | Activation | candidate_activation | yes |
| J08 | Activation | candidate_activation | yes |
| J09 | Activation/maintenance | candidate_activation_maintenance | yes |
| J10 | Activation/maintenance | candidate_activation_maintenance | yes |
| J11 | Consolidation | candidate_consolidation | yes |
| J12 | Consolidation | candidate_consolidation | yes |
| J13 | Maintenance | candidate_maintenance | yes |
| J14 | Maintenance | candidate_maintenance | yes |
| J15 | Maintenance | candidate_maintenance | yes |
| J16 | Maintenance | candidate_maintenance | yes |
| J17 | Intentions | candidate_intentions | yes |
| J18 | Scoped harness | candidate_scoped_harness | yes |
| J19 | Workspace/harness | candidate_workspace_harness | yes |
| J20 | Scoped harness | candidate_scoped_harness | yes |
| J21 | Results | candidate_results | yes |
| J22 | Results | candidate_results | yes |
| J23 | Adaptive work | candidate_adaptive_work | yes |
| J24 | Adaptive work | candidate_adaptive_work | yes |
| J25 | Adaptive work | candidate_adaptive_work | yes |
| J26 | Adaptive work | candidate_adaptive_work | yes |
| J27 | User interaction | candidate_user_interaction | yes |
| J28 | User interaction | candidate_user_interaction | yes |
| J29 | Scoped work | candidate_scoped_work | yes |
| J30 | Formation | candidate_formation | yes |
| J31 | Investigation | candidate_decision | yes |

*(Re-run `npm run inventory` after changing a module or the catalogue; the tables
above are a snapshot, not a maintained duplicate — `packages/supervisor/test/inventory.test.ts`
asserts the script still lists all seven modules and 31 families.)*

## The Host: jobs, sessions, budgets

The Host is a Rust HTTP service (`crates/memory-host`) that owns jobs, budgets,
effects and application completion; Pi owns its own persisted operation state. A
Pi result alone never marks a Host job complete.

**Submit → claim → start → complete.** An administrator stores the domain
references a `WorkBrief` needs and creates its root budget; a client submits a
job. A worker (or, for a pool, a `pool`-role credential calling `claim_next`)
claims it and receives an owner, epoch and expiring lease; `issue_worker_credential`
mints a job-bound token scoped to exactly that job and its deadline. The worker
calls `start`, drives the work (renewing its lease, reserving each provider
request, replaying or reconciling every tool effect through one `wrapTool`
boundary), then persists its result and a stable request ID before submitting
`complete`. The Host validates it, reserves final output, publishes a readable
result artifact, then atomically commits the job result, receipt and event.

**Recovery.** The Host's own timer runs coordinator `recover` every five ticks
(`RECOVERY_TICKS` in `crates/memory-host/src/main.rs`) for each administrator
scope: an expired session lease returns its job to `queued` with the attempt
count intact; an in-flight effect whose owner is gone becomes `outcome_unknown`
and blocks completion until `reconcile_effect` records the real outcome; a job
past its deadline or attempt limit is marked `failed`. Nothing manual is needed
for an ordinary worker crash — see
[runbook 1](runbooks.md#1-worker-crash--exercised) and
`packages/supervisor/test/continuous.test.ts`, which kills a real pool child
mid-job and restarts it to prove this end to end.

**Sessions.** `openLocalSession` uses the pinned Pi SQLite backend with a kernel
file lock; `PgSessionRepo` uses the fenced PostgreSQL backend, checking the
coordinator's lease and epoch on every mutation so a superseded epoch cannot
write. A job's `session_id` can hold only one active lease at a time — a job
scheduled with the same session as a still-open, interactively held job cannot be
claimed until that lease is released or expires.

**Scope.** Every write and read carries a trusted `Authority` and `Scope`
(user/project/task/entity/source-revision dimensions). A grant dimension of
`None` is unrestricted within the tenant; a grant of `Some(x)` can only reach a
resource whose own value is exactly `Some(x)`, never an unset one — a
freshly-scoped task cannot write a project-wide record, and only formation's
`destination` authority override can.

**Budgets.** Reservations charge their declared maximum until a job settles; an
unknown bill from a provider that reports no cost telemetry is never treated as
zero. `budget_usage` reports committed spend and unresolved reservations
separately.

## Composition, formation and activation

**Investigation** attaches the assigned Pi worker, constrains its reply to the
`WorkResult` contract, and delivers granted input memories through
`current_memories`. A granted memory that has since been revised or withdrawn
settles the job `blocked` with the reason, rather than silently working from a
superseded input.

**Formation** (`packages/formation`) reads a settled task's own published
experience — or any trusted connector's `memory-tool-events/1` source — one
bounded window at a time, asks J01/J02 (and J03–J05, J27, J30 as applicable) what
each candidate is and whether the evidence actually supports it, and retains
episode, knowledge, procedure and intention candidates that pass. It never
revises earlier records on its own; a correction or later disagreement is
maintenance's job.

**Activation** (`packages/activation`) retrieves by explicit identity, lexical
search (SQLite FTS5 / PostgreSQL text search) and, when a query embedding is
supplied, exact cosine scoring, then uses J06–J08 to check whether each
candidate's evidence role and prerequisites actually apply before it enters the
task's context. Local embeddings use Nomic Embed Text v1.5 through
Transformers.js (`npm run embeddings`); indexing is a separate administrator
operation, so activation can encounter records that are not yet embedded and
reports that gap rather than pretending coverage is complete.

## Consolidation, qualification and adoption

`runConsolidation` groups memories that share a source into a cohort, proposes a
conditional method (a concise summary, common mechanism, variations, untested
cases), and uses J11/J12 to check comparability and preserved conditions before
retaining it as a candidate. Retention is **not** qualification: the candidate
stays a candidate until it passes qualification.

`start_qualification` creates an `evaluation` child job carrying the candidate's
review artifact; `runQualification` (`packages/consolidation/src/qualification.ts`,
driven from the Host's `qualification.rs`) runs the held-out cases through the
original episodes, a concise summary and the candidate method, and `adopt_procedure`
compares the trial report against the configured success/regression/cost rules.
Only a comparison that passes those rules revises the candidate to `evaluated`;
a failed comparison preserves it as a candidate and returns why.

## Maintenance and intentions

`runMaintenance` (`packages/maintenance`) takes a bounded request — the current
record, a proposed revision or a removed source, a reason — and uses J13–J16 to
distinguish wording changes from a genuine world change, reassess remaining
support and check indirect dependents, before committing a revision that closes
the old valid interval and adds a successor (never erasing history). Source
removal retires guidance that depended on it while leaving independently
supported claims available.

An **intention** carries an optional `IntentionPlan` (trigger, execution brief,
readiness evidence, completion rule, optional recurrence). The Host sweeps
occurrences on its own timer, arms and fires them on their configured trigger
(a due time, a resource event, a new source revision, a completed job, or a
configured semantic question), and only marks one `completed` against a real
WorkResult, matching scope/inputs and satisfied effects — never on silence.

## The decision attachment

`packages/supervisor/src/decision.ts` offers the `decide` application tool
(module id `decision`, no `operations` of its own — it attaches wherever
`judgement` is installed). It builds a bounded, deterministic menu of next
operations from the task's own observations (`continue`, `inspect_further`,
`apply_procedure`, `retrieve_more`, `investigate_conflict`, `return_unresolved`),
asks J31 to pick one, and the Host rechecks that choice against its own current
state (`check_decision`) before anything acts on it — a model's pick is a
proposal, never itself authority.

## Schedulers: the Host's own triggers

The Host schedules work on its own timer (`crates/memory-store/src/scheduling.rs`,
driven from `crates/memory-host/src/main.rs`); nothing here requires an operator
to submit a follow-up job by hand. Every scheduling decision is recorded once in
the `scheduled_work` ledger (`tenant_id, trigger_kind, trigger_id`, deduplicated
by a `dedupe_key` so an observed trigger never double-submits), naming the job it
produced or, if none could be scheduled yet, why not — so a disabled module's
triggers accumulate as *scheduled-but-unclaimed* work, never silently lost.

- **Formation** (`Store::schedule_formation`, `crates/memory-store/src/experience.rs`):
  a settled task's own published experience schedules a `Process::Formation` job
  over that source revision.
- **Cohort consolidation** (`schedule_consolidation`): once a policy's configured
  `min_cohort_size` is reached for a shared source, a `Process::Consolidation`
  job is scheduled over that cohort (`inputs.memories`).
- **Qualification** (`schedule_qualification`): a retained candidate procedure
  schedules a `Process::Evaluation` job. The evaluation module first looks for a
  real `QualificationTask` manifest artifact; when the trigger carries no
  manifest, it derives one from the brief's typed `scheduled` field (candidate
  = `inputs.memories[0]`, held-out cases from the consolidation review that
  produced it) and otherwise falls back to a `partial` result naming the
  candidate with a clear unresolved line — it never fabricates cases.
- **Maintenance** (`schedule_maintenance`): a correction or a source revision
  schedules a `Process::Maintenance` job for each affected record, carrying a
  typed `scheduled: ScheduledInputs::MaintenanceCorrection | MaintenanceSource`
  field with a real `MaintenanceRequest` (including an `after` revision, since
  `review_maintenance` requires either a proposed `after` or a removed source —
  a bare "revalidate, no change proposed" request is rejected). The job's own
  scope matches the affected record's scope, not the triggering policy's scope,
  since a freshly-scoped job cannot write into a differently-scoped record.

`WorkBrief.scheduled` (`crates/memory-domain/src/contracts.rs`) is the typed seam
for this: an optional, boxed `ScheduledInputs` enum carrying exactly the
structured trigger data a scheduled job needs, so a scheduler never has to smuggle
JSON through a purpose string for the pool module on the other end to parse back
out. `packages/supervisor/test/scheduled-inputs.test.ts` covers the
maintenance-manifest derivation and the evaluation fallback in isolation;
`packages/supervisor/test/learning.test.ts` waits for the Host's own scheduler
(polling `events` for `job_accepted`, as `scenario.test.ts` does for formation)
rather than hand-submitting these jobs, so its "unattended learning" proof —
formation → cohort consolidation → qualification → adoption → later activation
retrieves the method → a correction schedules maintenance that completes →
intention fires and settles — is genuinely unattended.

Adoption (`adopt_procedure`) stays a deliberate interactive action rather than
something the Host schedules: it needs a live fence on the still-open evaluation
job that produced the candidate, which a Host-side scheduler cannot supply on
the job owner's behalf.

## Retention, erasure and administration

Corrections, teaching, temporary explorations, decisions and notifications go
through the same authenticated `POST /v1/commands` endpoint as investigation and
formation, using `UserRequest`/`UserMutation`. Revocation and deletion
(`begin_deletion`, acknowledgements, `purge_deletion`) precede physical cleanup;
restores carry the deletion registry so a restored backup cannot resurrect
already-deleted material. See [runbooks](runbooks.md) for the operational
procedures and [storage](implementation/storage.md) for the underlying schema.

## Running it

```sh
cargo build -p memory-host                       # build the Host binary the TS integration tests spawn
npx vitest run packages/supervisor/test/compositions.test.ts   # the composition matrix (6a)
npx vitest run packages/supervisor/test/continuous.test.ts     # the headless continuous scenario (6b)
npm run inventory                                 # the capability inventory (6d.3)
```

The composition matrix (`compositions.test.ts`) composes and runs a small job
through each named composition and asserts the properties in
[Composition](#composition-services-modules-workerjson) above. The continuous
scenario (`continuous.test.ts`) spawns the real Host binary and the real
production entrypoint (`packages/supervisor/src/main.ts`) as separate OS
processes, driven only through a `worker.json` and the client API — a source-backed
task forms memory automatically, a later task retrieves it at admission, and a
third task is killed mid-flight and the pool restarted to prove recovery against
the real entrypoint rather than a test harness stub. Both use a
`judgement.primary.kind: "scripted"` / `task_model.provider: "faux"` route usable
only when `MEMORY_TEST_SCRIPTED=1` is set — a real `GenerativeProvider`
serialization path backed by a scripted model, never a production mock mode.

`evals/campaign/run.ts` is a paid-campaign scaffold (gated behind
`MEMORY_LIVE_CAMPAIGN=1`): it isolates arms (no memory / capture-only / full
learning) against an AEC-Bench-shaped task list through the `TaskAdapter` seam
(`packages/supervisor/src/adapter.ts`), holds out transfer tasks, and reports
task outcomes and cost from `budget_usage`, separated into operational
correctness, semantic quality and behavioural value. Its dry run
(`packages/supervisor/test/campaign-dry-run.test.ts`) exercises the same code
over three fixture tasks with no credentials; see
[`evals/campaign/README.md`](../evals/campaign/README.md).
