# Agent Memory

Agent Memory is a memory service for AI agents. It records what happened during a
task, keeps useful information with its sources and limits, and retrieves relevant
memories for later tasks. When information changes, it can revise the affected
memories while preserving their history.

Use it to build and test agents that learn across tasks. You can also use its CLI
to inspect memories, add information, correct a record, or record a commitment.
It is a developer project with an HTTP API, not a standalone chat application.

## How it works

A **Rust Host** stores memories, sources, jobs and budgets in SQLite or PostgreSQL.
It checks access, schedules follow-up work and recovers expired worker leases.
A **TypeScript worker pool**, built on Pi, runs the enabled processes:

- **Investigation** does a task and records its experience.
- **Formation** reviews that experience and creates candidate memories.
- **Activation** retrieves memories and checks whether they apply to a task.
- **Consolidation and evaluation** propose reusable methods and compare them
  against held-out cases.
- **Maintenance** rechecks memories after corrections or source changes.

Workers can also use a decision tool to assess possible next steps. Each job has
an explicit scope, input references and resource limits. You choose which
processes, model routes and execution tools the pool can use.

The model judgements are **experimental**. The repository has automated checks
for storage, access, scheduling and recovery, but those checks do not establish
that its models remember the right things or improve task outcomes. See
[release status](RELEASE.md) for the qualification limits.

## Try it locally

You need Rust/Cargo, Node **22.19 or newer**, npm, Python 3 and `tar`.
Run these commands from the repository root:

```sh
npm run setup
npm run build
npm run memory -- init /tmp/memory-demo
./target/debug/memory-host /tmp/memory-demo/host.json
```

Setup downloads the pinned Pi source, npm dependencies and public model metadata.
The first Rust build also downloads dependencies. Choose an instance directory
that does not already exist. The Host listens on `127.0.0.1:7331`; add
`--port 7332` to `init` if you need another port.

Leave the Host running. In another terminal at the repository root:

```sh
node examples/walkthrough.mjs /tmp/memory-demo seed
npm run memory -- browse --config /tmp/memory-demo/client.json
node examples/walkthrough.mjs /tmp/memory-demo correct
node examples/walkthrough.mjs /tmp/memory-demo inspect
```

This example stores an attributed statement about a wall, corrects it, and shows
the result. It uses the real database and API and needs no model credentials.
It does not start a worker pool; the example task stays queued. Follow the
[user guide](docs/user-guide.md) to try teaching, temporary explorations,
commitments and decisions.

The instance directory contains credentials. Keep it private. Stop the Host with
Ctrl+C; restart it with the same `host.json` to use the saved data.

## Run agents with memory

To add a worker pool to an initialized instance:

```sh
npm run memory -- enable-pool /tmp/memory-demo
```

Before restarting the Host, edit `/tmp/memory-demo/worker.json`:

1. Set `env_file` to a private file containing your provider configuration.
   The generated configuration uses Azure OpenAI for task work and Jev for
   judgement, so it requires `AZURE_OPENAI_API_KEY` and `JEV_API_KEY`, plus the
   provider's endpoint and model settings.
2. Choose `task_model`, the `judgement` routes and enabled `processes`.
3. Configure `execution` if tasks need native tools. Without it, tasks run
   without native tools.

Restart the Host to launch the pool. These workers can make paid model calls.
See the [runtime configuration](docs/runtime.md#composition-services-modules-workerjson)
for the file format and the [runbooks](docs/runbooks.md) for operation and recovery.

For your own application, submit a `WorkBrief` through the authenticated
`POST /v1/commands` API. A brief names the task, inputs, scope, policy and budget.
The [user guide](docs/user-guide.md) shows requests you can adapt; the
[contracts guide](contracts/README.md) explains the generated request and response
types. The default task validator checks reported coverage against the brief's
criteria; domain-specific correctness needs a task-specific validator.

## Run the checks

After setup:

```sh
npm run check
```

This runs formatting, Rust linting, builds, TypeScript checks, project tests and
Pi session tests. Default tests need no model keys or paid services. They use
local test providers and loopback listeners.

Useful focused checks:

```sh
npx vitest run packages/supervisor/test/compositions.test.ts
npx vitest run packages/supervisor/test/continuous.test.ts
npm run inventory
npm run test:store
```

The first two exercise different pool configurations and a running Host/pool
with crash recovery. `inventory` lists the modules and judgement questions.
`test:store` checks SQLite/PostgreSQL parity using a disposable local PostgreSQL
cluster; it also needs `pg_config`, `initdb` and `pg_ctl`.

## Read more

- [Runtime guide](docs/runtime.md): process composition, scheduling and the memory loop.
- [Storage](docs/implementation/storage.md): records, revisions and database design.
- [Runbooks](docs/runbooks.md): recovery, backup, deletion and upgrades.
- [Deployment](deploy/README.md): container configuration and its current limits.
- [Campaign fixture](evals/campaign/README.md): reporting checks and the work needed for live evaluation.
