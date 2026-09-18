# Upstream compatibility

The [machine-readable manifest](../../contracts/compatibility.json) records source
pins. The imported [design manifest](design-sources.json) records the earlier
source-only review; its `false` flags are historical, not current test results.

## Build and prerequisites

| Component | Selected baseline | Prerequisites / licence |
| --- | --- | --- |
| Rust domain | Cargo.lock; tested Rust 1.96.0 | Serde, UUID and Chrono: MIT or Apache-2.0; Schemars: MIT. |
| Pi | `e4c75a73222ae2c72abb5f5314fa35ee8effc508`; five source packages at 0.85.1 | Node >=22.19; MIT. Built from source, not assumed equivalent to an npm release. |
| Node SQLite | `node:sqlite`; observed SQLite 3.53.3 on Node 26.4.0 | No external SQLite daemon or native npm addon. SQLite version follows Node. |
| Test/schema tools | package-lock.json | TypeScript: Apache-2.0; AJV, AJV formats, JSON Schema to TypeScript and Vitest: MIT. |
| Harbor | `kobe0938/harbor` at `dd4784b1ade1f446399e194f6dffd15142a77b98`, package metadata 0.22.0 | Python >=3.12; Apache-2.0. Installed in `.venv-harbor`; complete Docker lifecycle exercised. |
| ASP | v0 SSH; reference branch at `8ee7f0188b4c55aeca47d90b430da24bbbdbda89` | Descriptor schema copied with licence. RFC and reference branch have separate revisions. |
| Model | Pi's `faux/faux-1`, pinned through Pi | Test-only scripted responses; no credentials or live provider requests. |

AJV is pinned at 8.20.0. Vitest and its coverage package use a 4.1.11 development
override to resolve the advisories reported by the initial install. Pi runtime
packages remain at the reviewed source revision. The final npm installation
reported zero known vulnerabilities.

The checked machine has Python 3.14.6 and OpenSSH 9.9p2. Harbor runs in a Python
3.13.2 virtual environment. The isolated `memory-wp05` Colima profile uses Docker
29.5.2. Daytona SDK 0.214.0 is installed through Harbor's approved provider extra.
The bounded remote profile passed on Daytona API v0.214.3; see the plan.

Pi omits generated model data from its source archive. Setup runs the shipped
`hydrate-model-data` script to read public provider catalogues. This does not call a
model. Those catalogue contents can change independently of the source pin; no live
model is selected from them for this increment. An already prepared checkout builds
offline. Updating a Pi pin requires replacing `.upstream/pi` and rerunning setup.

## Interfaces exercised

The worker compiles against these public exports:

```ts
AgentHarness.create({ session, models, model, tools, toolContext, activeToolNames }, context)
harness.lane("main", context)
lane.accept({ kind: "prompt", operationId, prompt }, context)
lane.drive({ operationId }, context)
lane.inspectExecution(context) // current.id identifies admitted work
lane.getResult(operationId, context)
lane.requestAbort(operationId, context)
lane.findEntries({ order: "newestFirst" }, context)
harness.close(context)
```

`createReadTool`, `createWriteTool`, `createEditTool` and `createBashTool` compile
against `ExecutionToolContext`, whose `env` is a supplied `ExecutionEnv`. Read and
bash execute in the smoke tests. `NodeExecutionEnv` is explicitly supplied by those
tests; the worker has no automatic local-environment fallback. This local environment
is not a security sandbox.

`SqliteSessionRepo({ directory, databaseFactory: createNodeSqliteFactory() })`
creates and reopens sessions. `createModels`, `fauxProvider` and
`fauxAssistantMessage` supply a controlled provider at the model boundary. Tests
admit and drive an operation, read its result, close/reopen SQLite, and inspect the
persisted result without another model call. Other tests cover pre-drive abort and
an unavailable model, and a blocked attempt to read protected evaluator facts.
WP04 now adds assigned-worker recovery and fenced local/PostgreSQL storage.
See the runtime guide for tested operation states and remaining qualification.

## Upstream and project responsibilities

| Upstream capability | Project implementation and remaining boundary |
| --- | --- |
| Process-local Session, lanes and operation state | Assigned worker maps host admission to a Pi operation; there is no raw remote Session façade. |
| SQLite and conformance entrypoints | OS-locked local integration and a PostgreSQL adapter with 56 checks, including workspace restoration. |
| Hooks, entries and native tools | Provider reservations, guarded payload checks, effect receipts, typed workspace checkpoints, render manifests and compaction are implemented. Selective erasure is WP13. |
| `ExecutionEnv` | ASP file/shell adapter tested through OpenSSH and the complete Docker and bounded Daytona lifecycles. |
| Usage and event interfaces | Coordinator budget ledger and scoped events implemented; production telemetry remains later work. |
| ASP descriptor schema | Trusted descriptor validation, endpoint fencing and provider receipt/reconciliation are implemented. |

Pi's persisted harness format is 4 and pre-stabilisation at this source. Source pins
are ordinary dependency compatibility controls; memory identities and lineage do
not use content hashes.

## Verification

`npm run check` runs formatting, Rust Clippy with warnings rejected, the source
build, schema generation, TypeScript checking, project tests and the upstream tests.
The upstream entrypoints are the shipped `test:session:conformance` script in the
agent package (54 tests), and the SQLite package's `test` script (105 tests,
including storage and repository conformance).

On 17 September 2026, `npm run check` passed in the working folder and a separate
installation under `/tmp`: 10 Rust tests, 21 TypeScript tests and 159 upstream tests
passed (190 total). Formatting, Clippy, source builds and TypeScript checking also
passed. Setup was exercised without pre-existing Pi source or node_modules; Cargo
used the machine's dependency cache. The final lockfile was then reinstalled and
the full check repeated in that separate copy.

The project suite covers both language representations of the same fixtures,
scope/deadline/result boundaries, ASP descriptors, and real Pi/SQLite plumbing with
scripted responses. That baseline did not measure semantic quality, remote execution or
production readiness. T01, T03 and T15 grow with later work packages; their complete
release scope is not claimed here.

Source references: [Pi source](https://github.com/earendil-works/pi/tree/e4c75a73222ae2c72abb5f5314fa35ee8effc508),
[ASP RFC](https://github.com/harbor-framework/harbor/pull/3023),
[ASP reference](https://github.com/kobe0938/harbor/tree/8ee7f0188b4c55aeca47d90b430da24bbbdbda89/asp).


## WP02 storage dependencies

SQLx 0.8.6 supplies the shared SQLite/PostgreSQL implementation. object_store 0.14.2
supplies multipart artifact I/O; its local filesystem backend is configured to sync
completed writes. Cargo.lock records all resolved versions. CSV ingestion uses the
CSV library. No custom object-storage protocol or parser is introduced.

`npm run test:store` runs shared SQLite/PostgreSQL domain suites and the Pi
PostgreSQL conformance checks in a private disposable PostgreSQL 14.17 cluster.
All passed after the approved orphan shared-memory cleanup. Cloud artifact storage
remains unqualified; see [storage](storage.md).

## WP03–WP05 runtime dependencies and evidence

Axum 0.8.9 supplies HTTP routing; the existing Tokio runtime enables networking and
shutdown signals. `pg` 8.23.0 supplies PostgreSQL access for Pi. Both production
additions were approved by Theo. Cargo.lock and package-lock.json record resolved
versions. POSIX local ownership and the remote helper use Python's standard library;
SSH uses the installed OpenSSH client. No custom database driver was introduced.

The full project checks now cover 15 Rust tests, 34 TypeScript tests and 159 upstream
tests, plus the separate real host/worker integration and 16 Python tests. The
Harbor environment runs four additional offline Daytona checks (20 Python total).
PostgreSQL runs 6 shared store tests and 55 Pi checks. The opt-in real OpenSSH suite
passes two tests. These suites overlap; counts are not unique-test totals.

The pinned Harbor package passed the complete Docker and bounded Daytona lifecycles. The Daytona
adapter uses public SDK lookup and a small Harbor compatibility subclass for
noninteractive reconnect, unique names, TTL and bounded command polling. These
hooks are specific to Harbor 0.22.0 and require requalification on upgrade. The
[runtime guide](runtime.md), [bridge guide](../../services/harbor-bridge/README.md)
and [plan](plan.md) record live provider evidence and the remaining limits.


## WP06 context and hook behavior

The pinned Pi implementation catches exceptions from `transform_context`,
`before_request` and `before_payload`, reports `handler_error`, and can continue.
Application admission therefore cannot rely on throwing from those hooks alone.
The worker latches those errors and guards `Models.streamSimple`,
`Models.streamDeferred`, and the returned `onPayload` callback outside Pi's hook
aggregation. Tests verify no provider call after context, reservation, access or
payload rejection. Closing and reattaching the worker clears the failed instance.

Default Pi compaction does not project arbitrary custom entries into its summary.
The workspace supplies `before_compaction` with a structural checkpoint and
retained custom results. `before_navigation` avoids importing an abandoned
branch's conclusions. Tests cover normal projection, compaction, tree navigation
and restoration. Plain Faux does not call `onPayload`; a test transport wraps it
to exercise the actual callback, including provider-added size and access races.
No live provider acceptance or cache performance is inferred from that transport.


## WP07 execution interfaces

The existing pinned Pi tool interface supports the `python` tool with durable
invocation memos and `replay: never`. Host effect receipts separately prevent an
uncertain execution from running again. Independent child investigations use Pi
sessions, not concurrent drivers of the parent's lane. Typed investigation plans
and host messages are generated from Rust alongside the existing contracts.

The interpreter uses Python's standard library and the existing ASP SSH transport.
It requires POSIX Unix sockets, file locks and a fork-capable sandbox. Linux runs
inside the existing protected Harbor image; macOS runs only the local protocol
fixtures. No production dependency was added. Rebuild sandbox images after this
change: they now include `/opt/memory/interpreter.py`.

WP07's Docker and local SSH checks cover the new interpreter path. The earlier
Daytona qualification covered WP05; WP07's changed Daytona image has not been
run against the paid service. Live model semantics remain unqualified.

## WP08 Jev question and transport contract

Reviewed TypeSafe's official [question guidance](https://docs.typesafe.ai/primitives),
[workflow guidance](https://docs.typesafe.ai/concepts/how-to-build-with-system-one),
[parallel-question cookbook](https://docs.typesafe.ai/cookbooks/parallel_questions),
[skill-suggestion cookbook](https://docs.typesafe.ai/cookbooks/skill_suggestion)
and [API reference](https://docs.typesafe.ai/api) on 18 September 2026.

The adapter posts `state`, `model` and `questions` to `/v1/systemone`. It uses
Node's existing fetch and Pi's installed `Models.completeSimple`; no production
dependency was added. Provider retries are disabled inside the Pi route so that
the runtime owns attempt accounting. Jev's documented retry statuses are 429 and
529. Authentication and packet-validation errors are not retried.

The catalogue follows the guidance to ask one bounded question per property,
include the full question in `instructions`, reference structured evidence by
path, and describe each alternative. Question IDs are response keys, not model
instructions. Overlapping properties become separate questions. Narrow requests
use short strings; structured instructions are not needed for the current wording.

Most catalogue questions use Choice with explicit missing-evidence and, where
needed, outside-category outcomes. This distinguishes a supported negative from
material that was never supplied. Noul remains appropriate when a proposition
probability alone is sufficient; 0.5 must not be relabelled as an explicit finding
of missing evidence. Score requires an ordered described rubric. Validation checks
Noul's range without adding confidence, and checks Choice/Score distributions,
selected alternatives, Score legends and weighted means. Distribution confidence
is retained as returned, not interpreted as calibrated correctness.

Compatible independent questions are batched in one call. The cookbook's
recommendation to ask speculative questions does not override our task selection,
disclosure grants or budget. A second request is appropriate when an earlier
answer supplies new evidence or determines the next options. The skill-suggestion
cookbook demonstrates this with a shortlist followed by fuller evidence.

Local scripted HTTP and Pi tests establish wire behavior, lineage and policy
separation. They do not establish live Jev accuracy, latency, pricing or calibration.
The catalogue has no enabled semantic qualifications. Provider response fields and
stable release identity must be checked again during paid qualification.

The live smoke test found a model-name mismatch: `jev-1.12` from the cookbook
returned HTTP 400 (`Unknown model`). `jev-latest` succeeded and reported
`jev-1.13.0`. Three typed Choice answers agreed between a combined request and
two separate-family requests. Two differed from the authored expectations.
J01/J02 revision 2 clarifies evidential wording and mutually exclusive support
alternatives. See the retained
[request/response report](../../evals/judgement/live-smoke.json). The successful
calls reported 4,012 input and 469 output tokens. Their estimated cost is
US$0.000168504 at the documented price, not an observed invoice; the rejected
request returned no usage. All families remain semantically unqualified.

The revised wording was subsequently tested in three requests on the same
synthetic example. Both runs returned `jev-1.13.0`. Revision 2 returned `inference`,
`yes` and `both` in the batch and in the separate-family calls, matching all three
authored expectations. The batch probabilities were 0.69, 1.00 and 0.90 respectively;
these are model outputs, not measured correctness rates. See the
[revised report](../../evals/judgement/live-smoke-revised.json). It reported 4,263
input and 451 output tokens across the three calls, approximately US$0.00018 at
the documented price. This wording check does not establish domain qualification.

## WP09 capture and import boundaries

Formation adds migration 006, `FormationWindow` / `FormationResult` schemas and the
internal `formation_window` / `commit_formation` host commands. Both commands are
bound to a current worker assignment. The existing typed record format and WP08
provider interfaces are reused; no production dependency was added.

The capture adapter accepts `memory-tool-events/1`; formation requires a typed
`FormationInput` in each selected event. The ATIF connector validates with the
installed Harbor models, which accept ATIF-v1.0 through ATIF-v1.8. A source identity
is supplied explicitly because ATIF session IDs can be shared by multiple
trajectory documents. Unknown versions fail validation. Source owners remain
responsible for truthful event metadata and native locator registration.

The current profile imports structured captures and preserves image/audio/model
references. It does not run a visual interpreter. It keeps explicit user attribution,
synthetic status, copied-context warnings and unknown telemetry. Nested trajectories
are separate inputs. The 1 MiB source and 64 KiB window bounds are deliberate local
limits; larger inputs must be divided into source windows by their connector.

WP09's first live Azure reference pass exposed JSON/answer-form failures: 10/20
responses failed the answer contract. Jev returned three probability sums of
0.99 that the 0.001 tolerance rejected. The [live diagnostic](../../evals/formation/live/README.md)
separates these transport/representation issues from semantic decisions.

The approved follow-up permits probability totals from 0.99 to 1.01 in both Rust
and TypeScript, including a small floating-point allowance. Values are preserved;
individual probability bounds and the 0.001 choice/score consistency checks remain
unchanged. Offline revalidation admits all 20 saved Jev replies, with expected
retention decisions in all 20 cases. No additional paid calls or writes were made,
and no qualification gate or active provider route changed.

The Azure reference now sends a strict `text.format` JSON schema through Pi's
existing `onPayload` hook. The schema is built from the packet's selected questions:
it requires all answer keys, permits only declared choice labels, fixes each answer
type and score legend, and disallows extra fields. Its size counts toward the
request allowance. Other Pi provider APIs keep their existing request path.
Numeric bounds and semantic checks remain in the local validator because Azure's
[supported schema subset](https://learn.microsoft.com/en-us/azure/foundry/openai/how-to/structured-outputs)
does not support numeric minimum/maximum keywords. Incomplete output remains
invalid even if its partial text happens to parse. API rejection does not trigger
an unstructured fallback or an adapter retry.
