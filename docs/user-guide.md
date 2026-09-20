# Run the memory system and try a use case

This guide starts a local Host and uses the CLI to retain a user statement, correct it, teach from a demonstration, explore a hypothesis and request a decision. It uses the real SQLite store and authenticated HTTP API, and needs no API keys, model calls or paid services. To see judgement, formation and the pool's automatic scheduling work the full loop unattended, see [Running it](runtime.md#running-it) in the runtime guide.

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

Automatic execution requires an intention plan, checked readiness and a worker. Confirmation alone cannot satisfy missing completion evidence. See [maintenance and intentions](runtime.md#maintenance-and-intentions) in the runtime guide.

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

## Beyond the CLI: judgement, formation and the pool's own scheduling

The steps above never run the worker pool. To see formation and activation judge findings
through the configured judgement routes, and the Host's own scheduler carry consolidation,
qualification and maintenance forward unattended, `npm run memory -- enable-pool DIRECTORY` adds
a worker pool to this same instance (see the
[runtime guide](runtime.md#composition-services-modules-workerjson) for `worker.json`), and
`packages/supervisor/test/continuous.test.ts` and `packages/supervisor/test/compositions.test.ts`
exercise that pool end to end — the former against the real production entrypoint, killing and
restarting the pool child mid-scenario to prove recovery; the latter across every supported
composition. Both are ordinary `vitest` runs with a scripted judgement/task-model route; neither
needs credentials or paid services (see [Running it](runtime.md#running-it)).

## Stop, restart and run checks

Stop the Host with Ctrl-C. Restart it with the same `host.json`; records, histories, pending decisions and notification preferences remain in SQLite. Use a newly initialized directory for a fresh exercise.

```sh
npx vitest run packages/cli/test/ packages/supervisor/test/
cargo test -p memory-store --test interaction
cargo test -p memory-host
npm run check
npm run test:store
```

The first command executes this guide's steps through a disposable Host, including restart, and runs the pool composition matrix and the headless continuous scenario against the real production entrypoint (see the [runtime guide](runtime.md#running-it)). Store tests check scopes, retries, owner decisions, dispatch controls and deletion; the Host tests cover HTTP source publication, runtime credentials, pool claiming and child supervision. `npm run check` also runs the existing Pi, Rust, bridge and upstream checks. `test:store` additionally needs PostgreSQL binaries and checks both storage backends in a disposable cluster. None of these commands use paid providers.

Use `npm run memory -- --help` for CLI commands. The running Host serves `/openapi.json`, `/schemas/host-request.json` and `/schemas/host-response.json`; [generated TypeScript contracts](../contracts/generated/host-request.ts) describe the same request types.
