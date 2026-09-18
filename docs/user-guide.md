# Run the memory system and try a use case

This guide starts a local Host and uses the CLI to retain a user statement, correct it, teach from a demonstration, explore a hypothesis and request a decision. It uses the real SQLite store and authenticated HTTP API. Part 1 needs no API keys, model calls or paid services. [Part 2](#part-2--watch-a-model-the-judgement-providers-and-memory-work-together) continues on the same instance with a live model, Azure and Jev judgement and the memory those produce; it costs about a cent per run.

The example concerns wall W-101. Its values are invented for the exercise. The system records them as attributed user statements, with source verification still open.

## 1. Build the repo

You need Rust/Cargo, Node 22.19 or newer, npm, Python 3 and `tar`. From the repository root:

```sh
npm run setup
npm run build
```

Setup downloads the pinned Pi source, locked dependencies and public model metadata. If you have already set up this checkout, start with `npm run build`.

## 2. Create a local instance

Choose a directory that does not exist yet:

```sh
npm run memory -- init /tmp/memory-demo
```

This creates three private files:

- `host.json`: the SQLite path, artifact directory, listener and credentials.
- `client.json`: a user credential scoped to the example project.
- `admin.json`: a separate tenant administrator credential.

The default address is `http://127.0.0.1:7331`. Use `--port 7332` at initialization if that port is occupied. Credentials expire after 30 days. Keep these files private; do not commit them or share the example directory.

## 3. Start the Host

In a terminal at the repository root:

```sh
./target/debug/memory-host /tmp/memory-demo/host.json
```

Expect `Memory host listening on 127.0.0.1:7331`. Leave this terminal running. Use another terminal, also at the repository root, for the remaining commands.

The Host runs durable storage, authenticated commands and intention timers. It does not launch a model worker. A submitted job remains queued until a worker claims it.

## 4. Check your identity and seed the example

```sh
npm run memory -- identity --config /tmp/memory-demo/client.json
node examples/walkthrough.mjs /tmp/memory-demo seed
```

The seed step creates a retention policy, a bounded budget, the statement “The user reports W-101 as 60 minutes”, and a job referencing that statement. Only policy and budget creation use the administrator credential. The remaining example actions use the client credential.

Every example step saves its request and response as readable JSON in `/tmp/memory-demo`. `walkthrough.json` holds the returned references. The requests are normal API requests: the example driver runs the CLI, which calls the Host.

## 5. Inspect the evidence and task

```sh
node examples/walkthrough.mjs /tmp/memory-demo inspect
npm run memory -- browse --config /tmp/memory-demo/client.json
```

The memory is at revision 1, has `evidential_status: "attributed_statement"` and is a `candidate`. Its coverage says that only a user statement was examined.

The task has no recorded render manifests. Inspection says so explicitly. A submitted brief is not evidence that a model read or used anything.

With an assigned Pi worker, task inspection returns its recorded selection, source coverage, conflicts, deferred context, projected messages and reported usage. Selection alone does not establish that the model relied on a particular item. Task context is paginated in groups of three manifests; pass the returned `next_manifest` with `--after-manifest UUID`.

## 6. Correct the statement

```sh
node examples/walkthrough.mjs /tmp/memory-demo correct
node examples/walkthrough.mjs /tmp/memory-demo inspect
```

The statement becomes “The user corrects W-101 to 90 minutes” at revision 2. Revision 1 remains in history. The correction publishes a maintenance change notice, which the existing workspace refresh uses to mark affected unfinished work for rechecking. It does not independently verify the value or qualify methods derived from it.

Run `correct` again to exercise an acknowledgement retry: the saved request ID returns the earlier result. A different request using the stale revision is rejected with `request_conflict`.

For your own correction, copy `correction.request.json`, give the command a new request ID, set `expected` to the current memory reference, and edit the record and reason. Submit it with:

```sh
npm run memory -- call --config /tmp/memory-demo/client.json --file /path/to/your-correction.json
```

The Host attributes the write to the authenticated actor and enforces its scope. A correction cannot move an existing record into another scope. For a personal preference, create a knowledge record with your actor ID in `scope.user_id`; retain the project restriction when it applies. Access still follows the configured credential scopes: this is not a separate end-user login service.

## 7. Teach from a demonstration

```sh
node examples/walkthrough.mjs /tmp/memory-demo teach
```

This stores an episode: an empty instance lookup was followed by a successful type lookup. The episode preserves the correction, actions, outcome and lack of independent verification. It stays a candidate. Consolidation and procedure qualification are separate worker processes; recording a demonstration does not prove a reusable method.

## 8. Explore without adding a retained claim

```sh
node examples/walkthrough.mjs /tmp/memory-demo explore
```

The hypothesis is stored in a temporary exploration. The collection still has only the corrected statement and teaching episode. The hypothetical record has `evidential_status: "assumption"`.

Promote the selected contribution explicitly:

```sh
node examples/walkthrough.mjs /tmp/memory-demo promote
```

It now appears in the collection, still as an assumption and a candidate. Promotion does not turn it into an observation. Explorations expire after their declared deadline, at most 30 days after creation. Expiry denies access; physical deletion uses the existing retention API. This walkthrough does not run a background cleanup service.

## 9. Record a commitment

```sh
node examples/walkthrough.mjs /tmp/memory-demo commitment
```

The intention asks the owner to verify the reported value against a source. Its occurrence is `pending`: no executable plan or checked readiness was supplied. This is an honest open commitment, not a scheduled source check.

To amend or renew an intention, use a correction with its current reference and revised definition. To inspect, cancel or confirm an occurrence, use the existing `inspect_intentions`, `cancel_intention` and `confirm_intention` Host commands through `memory call`. For example, save this as a JSON file, replacing the occurrence ID shown by the commitment step:

```json
{
  "action": "cancel_intention",
  "occurrence_id": "REPLACE_WITH_OCCURRENCE_ID",
  "reason": "The owner no longer requires this check"
}
```

Automatic execution requires an intention plan, checked readiness and a worker. Confirmation alone cannot satisfy missing completion evidence. See [maintenance and intentions](implementation/runtime.md#maintenance-and-intentions-wp12).

## 10. Prepare a decision and test silence

```sh
node examples/walkthrough.mjs /tmp/memory-demo decision
node examples/walkthrough.mjs /tmp/memory-demo mute
```

The request records the question, missing authority, affected job, owner, deadline and fallback. Its answer remains `null`. The Host rejects a start, new model dispatch or new side effect for this job while approval is missing. Other jobs can continue. Work already dispatched cannot be recalled by this control.

Muting notifications returns an empty notification page. It does not answer the request, cancel the commitment or change the job. Only the named owner can answer before the deadline:

```sh
node examples/walkthrough.mjs /tmp/memory-demo answer
```

The answer becomes `approve`, with the owner's reason and identity. It permits continuation within the job's existing authority; it does not add capabilities. A decline or an expired unanswered request keeps the job blocked. Cancel that job and submit a revised brief if the decision needs to be reconsidered. Approval does not launch a worker.

Complete the decision step within 30 minutes of creating it. The example job deadline is one hour after seeding.

## 11. Read changes and choose notifications

```sh
node examples/walkthrough.mjs /tmp/memory-demo changes
npm run memory -- notifications --config /tmp/memory-demo/client.json
```

The change brief contains scoped activity events and details of revisions. Event kinds distinguish new contributions, corrections, promoted hypotheses, intention changes and decisions. Follow their `resource_id` with the corresponding inspection command to read the current state. Reuse `page.cursor` as `--after` to continue. When `snapshot_required` is true, refresh the current records and jobs before continuing from the new cursor.

Notifications use the same outbox, filtered by the actor's preference: `material` (default), `blockers`, `completion` or `muted`. Restore material notifications by saving this as `/tmp/memory-demo/notifications.json`:

```json
{ "action": "notification_preference", "mode": "material" }
```

```sh
npm run memory -- mutate --config /tmp/memory-demo/client.json --file /tmp/memory-demo/notifications.json
```

Delivery is a CLI/API pull. After handling an event, submit `{"action":"acknowledge_notification","event_id":"EVENT_UUID"}` with `mutate`. Acknowledged events are not delivered again to that actor. Polling itself does not acknowledge anything; a consumer should use the event ID to handle retries without repeating its own action. There is no email or chat delivery adapter in this increment.

## 12. Operator controls

Use `admin.json` for `create_policy`, `revise_policy` and `set_control` mutations. Policy updates require the current `ConfigRef`; jobs retain their assigned policy/profile references. Editing policy text does not qualify a model or a judgement family.

For example, disable a provider using its actual Pi provider name:

```json
{
  "action": "set_control",
  "kind": "provider",
  "target": "azure-openai-responses",
  "expected_revision": 0,
  "enabled": false
}
```

Save that payload as `/tmp/memory-demo/control.json`, then run:

```sh
npm run memory -- mutate --config /tmp/memory-demo/admin.json --file /tmp/memory-demo/control.json
npm run memory -- controls --config /tmp/memory-demo/admin.json
```

Use revision 0 to create a control; use its returned revision for the next update. `enabled: true` reopens dispatch. Controls can also target a judgement `family` such as `J16`, or a `profile` UUID. They apply to subsequent admissions, not in-flight requests. Only an administrator with the whole tenant scope can change them. Provider secrets remain in worker configuration, outside these payloads.

## Part 2 — Watch a model, the judgement providers and memory work together

The steps above never call a model. This part continues on the same instance and shows the
full loop: a model works a task from the memory you corrected, formation judges its findings
with Azure as the reference provider and Jev in shadow mode, and the CLI shows what memory
retained and why. It makes paid calls: the recorded run below cost about one US cent.

You need `.env` at the repository root with `AZURE_OPENAI_API_KEY`, `AZURE_OPENAI_BASE_URL`,
`AZURE_OPENAI_API_VERSION`, `AZURE_OPENAI_DEPLOYMENT_NAME_MAP` and `JEV_API_KEY`. The pool's task
model is `gpt-5.4-mini` (it must copy scope identifiers exactly) and its formation reference is
`gpt-4.1-mini`, the reference model in the repository's live evidence; both are set in
`worker.json`. Complete steps 4–6 of Part 1 first; the live steps read `walkthrough.json` and use
the corrected W-101 statement as the task input.

In this part nothing drives jobs by hand. The Host starts a **worker pool**: a Node child with a
`pool` credential that asks the Host for the next eligible job, receives a job-bound worker
credential with the assignment, drives the job and settles it. You submit work with the client
credential and watch two terminals: the Host terminal shows the pool's trace; the CLI shows the
recorded state. Every live step saves its requests and responses under `/tmp/memory-demo/live/`.

### 13. Enable the pool and prepare the task

```sh
npm run memory -- enable-pool /tmp/memory-demo
```

This adds a `pool` credential and a `worker_pool` section to `host.json` and writes `worker.json`
(models, concurrency, lease and poll intervals, the `.env` path). It is a configuration change, so
stop the Host with Ctrl-C and start it again:

```sh
./target/debug/memory-host /tmp/memory-demo/host.json
```

```
Memory host listening on 127.0.0.1:7331
Starting 1 worker pool child(ren)
pool 0: classes interactive,deferred; processes investigation,formation; task azure-openai-responses/gpt-5.4-mini; reference gpt-4.1-mini; jev jev-latest
pool 0: idle
```

Leave it running. In the other terminal:

```sh
npm run live -- /tmp/memory-demo prepare
```

```
Budget 6c1e…: at most 30 provider calls and 2.000000 cost units; reservations settle to observed usage.
Task job 01a0b3a4-10a3-… queued (investigation; input memory "W-101 fire resistance" revision 2). The pool claims it within a second.
Next: npm run live -- DIRECTORY work
```

The administrator credential creates a bounded budget; the client credential submits an
investigation job whose only input is the corrected W-101 memory. The Host terminal shows
`pool 0: claimed investigation job …` almost immediately.

### 14. Watch the model work the task

```sh
npm run live -- /tmp/memory-demo work
```

```
Task job 01a0b3a4-10a3-…: queued
Task job 01a0b3a4-10a3-…: running (attempt 1)
Task job 01a0b3a4-10a3-…: partial (attempt 1)
Recorded context (1 render manifest(s)):
  render responded (azure-openai-responses/gpt-5.4-mini); 3 selected: goal "Report the fire-resistance rating…", constraint "{"definitions":[…", memory "{"label":"W-101 fire resistance","content":{"family":"knowl…"; 0 source(s); 0 conflict group(s); 0 deferred; 7133 bytes

Result status partial; 2 finding(s)
  1. [observed/attributed_statement] The currently recorded fire-resistance rating for wall W-101 is 90 minutes.
     supported by W-101 fire resistance r2
  2. [agent_generated/inference] The evidence provided in the workspace is only a user statement, and no source file was inspected to verify the rating.
     supported by W-101 fire resistance r2
  examined:   Reviewed the supplied W-101 fire resistance memory.; Checked the workspace for additional sources or artifacts.
  unexamined: No source file or external evidence was available to inspect.; …
  unresolved: Verify the W-101 rating against a source file or other primary evidence.
Budget ledger: committed 1 call(s), 3054 tokens, 4522 cost microunits; unresolved reservations 0 call(s), 0 tokens.
```

Read this in order. The pool claimed the job and received a credential bound to it. Granted
input memories enter the workspace as label-only entries, so the worker delivers their current
content through the fenced `current_memories` command before rendering; the render manifest then
records exactly what the model received (three entries, 7,133 bytes). The provider is asked for
the `WorkResult` contract as a strict output schema, the same seam the judgement reference uses,
so the shape cannot drift; the Host still validates the content before committing. A result
cannot be `complete` while anything is unexamined or unresolved, which is why this one is
`partial`. The first finding keeps the memory's status — an attributed user statement, not an
observation — and the model's own conclusion is marked as an inference. The number and wording of
findings vary between runs. The budget ledger, not the model's self-reported usage, is the spend
record.

If you look at the Host terminal you will also see the pool claim the job that Part 1's `seed`
step submitted, and settle it as blocked: that brief granted the W-101 memory at revision 1, and
you corrected it to revision 2 in step 6. A worker does not silently work from a superseded input;
the job's result says which revision the brief granted and that it must be resubmitted.

Inspect the same manifest through the API with `npm run memory -- task --config /tmp/memory-demo/client.json --id JOB_UUID`.

### 15. Capture the findings as a source

```sh
npm run live -- /tmp/memory-demo capture
```

```
Published source a85bf41f-… revision live-1; snapshot artifact 01a0b3a4-…
  finding-1: [observed/attributed_statement] The currently recorded fire-resistance rating for wall W-101 is 90 minutes.
  finding-2: [agent_generated/inference] The evidence provided in the workspace is only a user statement, and no source file was inspected to verify the rating.
Each event keeps the finding's own origin and evidential status; the connector adds no verification.
Formation job 01a0b3a4-3a3d-… queued; it reads source a85bf41f-… revision live-1. Watch the Host terminal for the judgement trace.
Next: npm run live -- DIRECTORY form
```

Formation reads structured `memory-tool-events/1` sources, never raw transcripts. The example
driver acts as a trusted connector: each published finding becomes one capture event with the
finding as evidence and the job, model and objective as conditions. It publishes the source with
the client credential through `ingest_source`, then submits the formation job. The Host checks
that referenced sources exist at submission, so this job can only be created now.

### 16. Watch formation judge, with Jev in shadow

The Host terminal shows the pool working the formation job:

```
pool 0: claimed formation job 01a0b3a4-3a3d-… (attempt 1)
pool 0: → shadow jev (jev-latest) asked J01.assessment, J01.faithfulness, J02.assessment about "Formation of finding-1"
pool 0: ← shadow jev: J01.assessment=attributed_statement, J01.faithfulness=yes, J02.assessment=supports (HTTP 200, 765 ms, tokens in 5615 / out 225)
pool 0: → reference azure-reference (gpt-4.1-mini) asked J01.assessment, J01.faithfulness, J02.assessment about "Formation of finding-1"
pool 0: ← reference azure-reference: J01.assessment=attributed_statement, J01.faithfulness=yes, J02.assessment=supports (HTTP 200, 2119 ms, tokens in 3892 / out 49)
pool 0: → shadow jev (jev-latest) asked J01.assessment, J01.faithfulness, J02.assessment about "Formation of finding-2"
pool 0: ← shadow jev: J01.assessment=inference, J01.faithfulness=yes, J02.assessment=supports (HTTP 200, 320 ms, tokens in 5611 / out 224)
pool 0: → reference azure-reference (gpt-4.1-mini) asked J01.assessment, J01.faithfulness, J02.assessment about "Formation of finding-2"
pool 0: ← reference azure-reference: J01.assessment=inference, J01.faithfulness=yes, J02.assessment=supports (HTTP 200, 1451 ms, tokens in 3939 / out 48)
pool 0: job 01a0b3a4-3a3d-… source a85bf41f-…@live-1: retained 2, deferred 0
pool 0: job 01a0b3a4-3a3d-… partial
```

```sh
npm run live -- /tmp/memory-demo form
```

```
Formation job 01a0b3a4-3a3d-…: queued
Formation job 01a0b3a4-3a3d-…: running (attempt 1)
Formation job 01a0b3a4-3a3d-…: partial (attempt 1)
Result status partial; unresolved: Verify the W-101 rating against a source file or other primary evidence.; Unexamined: …
Retained 2 candidate record(s) from the task's findings:
  + 01a0b3a4-10a3-…: finding-1 r1 [knowledge, attributed_statement, candidate]
  + 01a0b3a4-10a3-…: finding-2 r1 [knowledge, inference, candidate]
The per-question Jev and Azure answers are in the Host terminal (pool trace). Jev ran in shadow mode and did not decide retention.
Budget ledger: committed 1 call(s), 3054 tokens, 4522 cost microunits; unresolved reservations 4 call(s), 120000 tokens.
```

Each candidate is one WP08 packet: J01 asks what kind of statement it is and whether it is faithful
to the evidence; J02 asks whether the evidence supports it. Both providers receive the same packet.
Only the Azure answer decides retention; Jev's answer is recorded alongside for comparison, and in
this run they agree. The retained records keep each finding's evidential status and link back to
the source. Providers report no monetary telemetry, so each judgement reservation stays at its
maximum in the ledger until the job settles; this is deliberate — an unknown bill is not treated as
zero. The formation job settles as `partial` because it carries the task's unexamined material
forward as unresolved work. If the Host restarts while a job is queued or running, the pool
restarts with it and coordinator recovery returns any expired lease to the queue; the recorded
run below includes such a restart between capture and formation.

### 17. See what memory now holds

```sh
npm run live -- /tmp/memory-demo show
npm run memory -- browse --config /tmp/memory-demo/client.json
npm run memory -- changes --config /tmp/memory-demo/client.json
```

```
Current records in scope (4):
  - W-101 fire resistance r2 [observed/attributed_statement, candidate] created by f1e7cc11-…
  - How the property was found r1 [observed/attributed_statement, candidate] created by f1e7cc11-…
  - 01a0b3a4-10a3-…: finding-1 r1 [observed/attributed_statement, candidate] created by 5e514c25-…; 1 source locator(s)
  - 01a0b3a4-10a3-…: finding-2 r1 [agent_generated/inference, candidate] created by 5e514c25-…; 1 source locator(s)
Task 01a0b3a4-10a3-…: 1 recorded render manifest(s); Up to three manifests per page; selection does not prove model use
Change events: memory_revised ×…, job_accepted ×…, job_leased ×…, job_settled ×…, source_registered ×1, …
```

The new records were created by the pool worker's actor, not by you, and carry a source locator
into the captured findings. They are candidates: nothing here verified the 90-minute value, and the
records say so. `history`, `task` and `changes` give the full detail.

### If a live step fails

Every step prints the failing Host command and the Host's reason; the pool logs each job it
claims, settles or fails in the Host terminal. A failed attempt shortens its lease so coordinator
recovery (every five seconds) can requeue it; after the brief's `max_attempts` the job fails. `npm
run live -- /tmp/memory-demo reset` cancels the live jobs and clears the live state (the budget and
any published source remain), after which `prepare` starts again. If `work` or `form` waits without
progress, check that the Host terminal shows `Starting 1 worker pool child(ren)` and no `cannot
start` errors: the pool needs `.env` credentials and the checkout it was configured from. The
recorded transcript of a full run, including the Host terminal, is
[evals/live-walkthrough/run-2026-09-18.json](../evals/live-walkthrough/run-2026-09-18.json).

### What this part does and does not show

It shows the real Host, worker pool, judgement and formation code paths with live providers, with
work claimed, driven and settled unattended. It does not run activation retrieval (only the
explicitly granted memory was delivered), consolidation or maintenance; the pool supports
investigation and formation so far and leaves other process kinds queued. It does not qualify any
model or judgement family, and Jev remains in shadow. The findings-to-events connector is example
code, not runtime.

## Stop, restart and run checks

Stop the Host with Ctrl-C. Restart it with the same `host.json`; records, histories, pending decisions and notification preferences remain in SQLite. Use a newly initialized directory for a fresh exercise.

```sh
npx vitest run packages/cli/test/ packages/supervisor/test/ examples/test/
cargo test -p memory-store --test interaction
cargo test -p memory-host
npm run check
npm run test:store
MEMORY_LIVE_WALKTHROUGH=1 MEMORY_LIVE_WALKTHROUGH_REPORT=evals/live-walkthrough/NEW.json npx vitest run examples/test/live-walkthrough.test.ts
```

The first command executes Part 1 through a disposable Host, including restart; runs the pool against a real Host with scripted providers (investigation, formation and a blocked stale-input job); and runs the offline tests of the pool's task helpers and the live driver's connector. Store tests check scopes, retries, owner decisions, dispatch controls and deletion; the Host tests cover HTTP source publication, runtime credentials, pool claiming and child supervision. The last command is the paid Part 2 run against a disposable Host and needs `.env` credentials and a new report path. `npm run check` also runs the existing Pi, Rust, bridge and upstream checks. `test:store` additionally needs PostgreSQL binaries and checks both storage backends in a disposable cluster. These commands use no paid providers.

Part 2 runs jobs through the Host's worker pool ([packages/supervisor](../packages/supervisor/src/index.ts)), which embeds the [assigned Pi worker](../packages/pi-worker/src/assigned-worker.ts) and the formation runtime. Production manifests, tenant fairness, backup and the remaining runbooks are still WP15 work. Web UI is outside this WP14 delivery.

Use `npm run memory -- --help` for CLI commands. The running Host serves `/openapi.json`, `/schemas/host-request.json` and `/schemas/host-response.json`; [generated TypeScript contracts](../contracts/generated/host-request.ts) describe the same request types.
