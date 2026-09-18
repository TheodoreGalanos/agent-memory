# Coordinator and Pi worker

The current runtime is a Rust HTTP host plus a TypeScript worker library. The host
owns jobs, budgets, effects and application completion. Pi owns its persisted
operation state. A Pi result alone does not mark a host job complete.

## Start a local host

Build with `npm run build`. Create a private configuration file with this shape,
using your own UUIDs, a random token of at least 32 characters and a future expiry:

```json
{
  "listen": "127.0.0.1:8080",
  "database_url": "sqlite:///absolute/path/host.sqlite?mode=rwc",
  "artifact_root": "/absolute/path/artifacts",
  "credentials": [{
    "token": "REPLACE_WITH_A_RANDOM_PRIVATE_TOKEN",
    "tenant_id": "00000000-0000-0000-0000-000000000001",
    "actor_id": "00000000-0000-0000-0000-000000000002",
    "scope": { "entity_ids": [], "source_versions": [] },
    "role": { "kind": "administrator" },
    "expires_at": "2026-12-31T00:00:00Z"
  }]
}
```

```sh
chmod 600 /absolute/path/private-config.json
cargo run -p memory-host -- /absolute/path/private-config.json
```

This administrative example has unrestricted scope within one tenant. Clients use
`{"kind":"client"}` and workers use `{"kind":"worker","job_id":"<uuid>"}`;
configure their scopes separately. Workers can only operate their assigned job.
Credentials come from the private host configuration, never from command payloads.
Rotation currently requires restarting the host. Remote access needs a TLS reverse
proxy; the binary binds only to loopback. Automatic credential issuance and a worker
pool are not implemented yet.

`POST /v1/commands` accepts the generated `HostRequest` union with a Bearer token.
`/openapi.json` links the served request/response schemas. `HostClient` is the typed
fetch client. Requests are limited to 2 MiB and concurrent command execution to 32.
The client bounds response reads to 2 MiB. Application results must also fit their
work brief and the artifact read limit of 1 MiB.

## Submit, execute and recover

1. An administrator stores the domain references required by a `WorkBrief` and
   creates its root budget. A client submits a `SubmitJob` command. The host assigns
   a stable Pi session and operation identity before any provider call.
2. A worker claims its job and receives an owner, epoch and expiring lease. Supply
   that assignment to `attachAssignedWorker`, along with `HostClient`, a trusted
   `WorkerProfile`, the matching session, model registry and execution environment.
   The profile reference and enabled tools must match the brief. Resources and
   system prompts are supplied through `WorkerOptions`; no ambient profile loader
   or automatic model selection is assumed.
3. `worker.run()` accepts or resumes that Pi operation. It renews ownership while
   driving, reserves each provider request and checks the final provider payload
   against the profile's byte limit. Retries and deferred provider handles become
   durable host waits, with the Pi state retained for the next assignment.
4. Native tools use host effect records. Reads can be replayed as observations;
   writes, edits and shell execution require receipts or reconciliation. Losing a
   response does not establish that a mutation failed. An unresolved effect keeps
   the host from accepting a complete result.
5. The worker persists the application result and a stable publication request ID
   before submitting completion. The host validates it, reserves final output,
   publishes a readable result artifact, then atomically stores the job result,
   receipt and event. A replacement worker can reuse the receipt after a lost reply.
6. For a sandbox worker, supply `ExecutionLifecycle` callbacks backed by
   `SandboxClient`. Renewal updates the protected endpoint; `finish` publishes
   declared exports and confirms deletion before job completion or cancellation
   acknowledgement. Close the worker and its session owner afterwards.

The executable example is `packages/pi-worker/test/host-worker.test.ts`, launched
by `crates/memory-host/tests/result_publication.rs`. It uses a real host, SQLite Pi
session, native read tool and scripted provider. It is an integration test, not a
production worker command.

Administrative `recover` processes expired leases and due waits. In-flight effects
become `outcome_unknown`; unresolved provider reservations retain their maximum
charge. `reconcile_effect` requires explicit resolution evidence. Cancellation
propagates to descendants and workers acknowledge it after stopping work.
`events` returns scoped cursors; an expired cursor requests a fresh snapshot.
`consume_event` atomically deduplicates event delivery and submits the resulting
job. `prune_history` removes eligible terminal history while retaining compact
request identities, so an old request cannot silently create new work.

These are callable operations; no background scheduler invokes them automatically
in this increment. The WP15 supervisor will own that loop.

## Session ownership

`openLocalSession(directory, sessionId)` uses the pinned Pi SQLite backend. A
kernel lock covers the canonical directory and session filename. Another process
cannot open it until the owner closes or dies. The lock helper requires Python 3
and POSIX `flock`; Windows is not qualified. The manifest checks worker family
`pi-0.85.1` and SQLite storage schema 1. Pi's JSONL format 4 is a separate version.
The returned backup operation uses SQLite's backup API. Migration from an
incompatible family is explicitly refused, rather than guessed.

For PostgreSQL, run the host migrations, then `migratePiPostgres(pool)`. Construct
`PgSessionRepo(pool, {tenantId, jobId, ownerId, epoch})` from the current assignment.
All session operations check the shared coordinator lease using database time;
mutations hold that lease row and recheck expiry before committing. Entries,
current values, lists, usage and metadata live in the `pi_sessions` schema.
`fork` preserves Pi graph/current-state semantics and resets operation state and
usage as required by the pinned upstream contract.

The caller supplies and closes its PostgreSQL pool. Session objects remain local
to the process; PostgreSQL is persistence, not a remote Session RPC service.

## Verification and limits

```sh
npm run check          # Includes real loopback host/worker integration
npm run test:store     # Disposable PostgreSQL; shared store and Pi conformance
npm run test:bridge    # Python helper and Harbor boundary tests
npm run test:asp       # Task-owned loopback sshd; no provider credentials
npm run test:docker    # Real host, Harbor, Pi/ASP, exports and provider deletion
```

The full project suite covers lost completion replies, retry/deferred resume,
initial cancellation, provider/tool receipts, local process death and ownership
transfer. PostgreSQL runs 55 storage/repository/fork/ownership checks. These results
do not qualify every possible Pi runtime state, a live model provider, distributed
failure/load behavior. Separate Docker and Daytona integrations qualify the documented sandbox profiles.

Current operational limits:

- Domain writes use a shared commit clock and are serialized. PostgreSQL claims use
  `SKIP LOCKED`, but no concurrent-throughput claim is made.
- Provider reservations depend on a trusted profile's token and price allowances.
  Unknown usage stays charged at the reserved maximum; post-terminal billing
  reconciliation is not exposed yet. Models cannot supply authoritative billing.
- The host protects final-result reserves from ordinary worker reservations. Result
  publication can leave a ready, unreferenced artifact if the final database commit
  fails; replay reuses it. An interrupted upload requires
  `ArtifactService.recover_upload` after confirming the old uploader has stopped.
- Full retention purge, live credential revocation, cloud artifact storage,
  continuous provider allocation supervision and production deployment remain later work.
- [Harbor/ASP](../../services/harbor-bridge/README.md) documents Docker evidence,
  the bounded Daytona profile and its separate qualification status.


## Workspace state and context

Assigned workers now create a typed workspace from the work brief. Pi custom
`memory.workspace` entries hold checkpoints; compaction carries the same state in
its structured details. The host does not mirror the transcript or workspace.
`worker.workspace.state()` reads the current lane's checkpoint, and
`worker.workspace.checkpoint(state)` publishes a trusted replacement through Pi's
normal write boundary. Checkpoints contain the complete working frame: preserve
any observation needed after raw conversation is discarded. Large evidence stays
in versioned source, memory or artifact references.

Each entry records its kind, origin, evidential status, scope, valid time, cited
inputs and supporting entries. Delegated findings name their generating operation
and remain distinct from evidence read by the parent. Task, phase and timed scratch
lifetimes control current use. `recovery_until` separately bounds workspace
recovery; physical session deletion remains WP13.

A temporary workspace can use permitted evidence, but `formationInputs` returns
only explicitly selected contributions. The WP09 connector carries this selection
into typed capture; the host still checks support before a collection write. `reconcileWorkspace` marks unfinished dependent
conclusions for revalidation after relevant substantive changes. Wording-only
changes, different valid-time intervals and completed reports preserve their cited
basis. The event scheduler and semantic change classification remain WP12.

Before each assistant request, the renderer:

1. Reads current access and scratch lifetimes. Assigned workers recheck their host
   lease and brief; a supplied `readAccess` callback can narrow that grant using
   current collection policy. It must change its revision when disclosure changes.
2. Keeps the goal, constraints, active obligations, decision-relevant uncertainty
   and complete unresolved conflict groups. If a conflict member is unavailable,
   the whole group and its dependent findings are withheld with an explicit notice.
3. Selects remaining entries by priority with stable ordering. It budgets system
   instructions, active tool schemas, model output and anticipated tool results.
   Optional evidence that does not fit is deferred with a narrower next step.
   If required meaning cannot fit, the request stops and records what must narrow.
4. Projects through Pi's transcript converter, validates tool-call/result pairs,
   and persists a `RenderManifest` in Pi bound values before the provider call.
   The manifest contains selected contents and references, source coverage,
   conflicts, deferrals, model/profile, tool schemas and the projected transcript.
5. Rechecks access and the final payload size through `before_payload`. Provider
   payloads and credentials are not persisted in the manifest. Only the final byte
   count is added. Transport fields can therefore increase size without leaking
   into the context inspection record.

`worker.workspace.latestManifest()` returns an access-checked task-context view.
When access has changed, it withholds the old contents until a fresh render exists.
The trusted session still contains its historical records; this is not erasure.
Revocations and substantive changes discard opaque conversation context because
old responses can paraphrase revoked evidence. The next request uses the current
structured workspace and clearly states that omitted details need inspection.

Stable instructions and unchanged working context stay at the front. Ordinary
turns append after them; context removal, reordering, source changes and budget
pressure record rebuild reasons and estimated/final sizes. Prompt cache usage is
separate from judgment or investigation reuse. Enable `usageMetrics` only for
fields the selected provider reports; otherwise values remain unknown.

The initial text profile uses UTF-8 bytes as a conservative token estimate, plus a
1 KiB provider margin and a configurable result reserve (1,024 tokens by default).
It is not an exact tokenizer. Images require a qualified multimodal profile and
are rejected by this text renderer. Provider-specific request acceptance, caching
and semantic quality still need live qualification. Tests use a scripted transport
that exercises the real Pi payload callback; plain Faux omits that callback, which
the manifest explicitly records.

Pi catches and logs most hook exceptions. `provider-guard.ts` prevents a failed
context, reservation or payload check from falling through to a provider call.
A rejected request becomes a failed Pi response and partial application work.
The guard stays closed for that harness instance; repair the cause and attach a
new worker. Compaction and branch navigation use project summaries so default Pi
summarisation cannot lose required custom content. Historical `memory.result`
entries retain their original scope through compaction. Navigating to another
branch uses its destination checkpoint and does not import the abandoned branch's
conclusions. A fresh child lane needs an explicit workspace checkpoint.

Sessions created before WP06 need an explicit trusted `memory.workspace`
checkpoint before attaching the workspace renderer. No inferred migration from
an arbitrary transcript is performed. The public low-level harness can open such
a session without workspace rendering to append that checkpoint.


## Scoped investigations (WP07)

Pass an `InvestigationPlan` as `investigation` to `attachAssignedWorker`. The
trusted caller supplies the method revision, interpretation conditions and a
bounded batch of questions. Each step identifies its evidence, permitted tools
and required output. `definitions` names JSON pointers into assigned artifacts;
those fields are expanded before child briefs are submitted. Missing fields stop
preparation with an explicit error. This is an application API, not an additional
model planner or a general workflow graph.

The host creates a separate job and Pi session for each question. A child can
narrow its parent's scope, evidence and tools, but cannot widen them or change its
policy, profile or disclosure rules. Depth and concurrency stay within the root
limits. The initial batch must fit the parent's child-concurrency allowance and
contains at most 32 steps. Children share the root budget; the root's final-result
reserve remains protected.

The parent returns `waiting` and releases its lease. The caller closes that worker
and schedules the children through the normal claim/attach path. `recover` wakes
the parent when its children finish; a new assignment reopens the same Pi session
and investigation state. This release consumes an assignment attempt, so the
parent needs at least two attempts for one investigation batch. Child deadlines
end two seconds before the parent's deadline. Cancellation uses the existing job
tree propagation and blocks new work. WP15 supplies deployment and scheduler
supervision; this library does not start a background scheduler.

Child results enter the workspace as delegated findings with their generating
operations. The parent retains their findings, missing coverage and unresolved
effects even if the generated synthesis omits them. An investigation artifact
records each question, method, evidence cutoff, applicability, coverage and result
artifact. Missing output is unexamined work. If combined findings exceed the
publication allowance, the result is explicitly partial and directs the reader to
the complete child artifacts. It does not claim that omitted evidence passed.
Required context that exceeds the renderer's allowance still requires a narrower
investigation. Output allowances cover intermediate artifacts and tool output as
well as the final result.

A step may name `reuse_job_id`. Reuse is explicit: the completed job must still be
retained and its brief must match the requested evidence versions, definitions,
method conditions, policy/profile, freshness requirement, cutoff and scope.
Execution limits may differ because reuse does not rerun the child. The host
rechecks current evidence and artifact access, including when returning a prior
spawn receipt. Failed, partial, missing and revoked results cannot become a cache
hit. There is no semantic cache lookup or content hashing.

`read_input` exposes bounded text from assigned artifacts, the job's own published
artifacts and linked child results. `inspectJson` resolves JSON pointers within a
64 KiB document. `publish_artifact` accepts up to 64 KiB of JSON, checks dependency
access and charges the root budget. The host publishes the final WorkResult before
committing job completion.

## Persistent sandbox interpreter (WP07)

Construct `InterpreterClient(aspEnv, host, assignment, sessionUuid)` and pass it as
`interpreter` with `python` in the worker's tool list. The brief must explicitly
permit `python`. Use the assigned Harbor/ASP sandbox; there is no host execution
fallback. The Pi tool exposes `start`, `execute`, `checkpoint`, `restore`, `receipt`
and `stop`. The same client API is available to trusted application code.

Start is explicit. Within that interpreter, Python variables persist across calls.
Each call permits 32 KiB of code and 8 KiB of printed output, with a three-second
wall limit. The supervisor kills an unresponsive interpreter process. It stops
an idle heap after 30 seconds and caps its lifetime at five minutes or the job's
deadline, whichever is sooner. The outer sandbox enforces memory, CPU, networking,
lease expiry and teardown. Its allocation already reserves compute against the
root budget; the interpreter reserves returned output separately. Returned byte
usage is observed by the worker; provider CPU billing remains with the allocation.

A checkpoint serializes only the named JSON-compatible application objects into
a scoped host artifact. Functions, handles, modules and the Python heap are not
checkpoints. After loss, explicitly stop/start and restore a checkpoint. If the
sandbox itself disappears, provision its replacement through the existing
lifecycle before restoration. The interpreter receives no host credentials.

An execution has both a host effect receipt and a sandbox receipt. A lost reply,
process exit or timeout leaves the outcome unknown and cannot automatically run
again. `receipt` can recover the sandbox's observation while it remains available;
a host administrator must reconcile uncertain effects before resuming dependent
work. A new request ID is not evidence that the earlier action failed. Checkpoints
recover selected data; they do not undo file mutations or settle uncertain effects.

## Semantic judgement (WP08)

`packages/judgement/src/index.ts` exposes the catalogue, packet builder, Jev and
Pi conventional-model adapters, and the assigned-job runtime. Select the relevant
families with `definition("J01")` etc.; the runtime does not run all 29 by default.
A family can contain several independent questions. For example, J19 checks
conditions, uncertainty, conflicts, obligations and checks separately.

Call `buildPacket` with the assignment, named artifact fields, selected questions,
provider disclosure grants and expiry. It reads the actual JSON values and the
host checks those values and their origin against the source artifacts. Missing
fields remain explicit. The packet retains source scope, cutoff, coverage,
question definitions and the job's policy and budget. Artifact-backed excerpts
carry their source lineage through artifact dependencies; empty source/record
lists do not claim additional inspected material.

Questions whose evidence and disclosure grants are compatible share one packet
and one provider request. Every question sees the whole shared state, so adding
a question cannot broaden disclosure. A dependent question goes in a later
packet with the preceding result supplied as inspected evidence. `depends_on`
names those evidence fields. Compare one candidate, prerequisite or coverage
item at a time; prepare another packet when the subject or required evidence
changes. These are practical bounds: 32 selected families/fields, 16 questions
per definition and 64 KiB retained packet/response artifacts.

Construct `JevProvider` with the model, a conservative per-attempt reservation
and a credential callback. Construct `GenerativeProvider` with the configured Pi
`Models` and model plus an output-token bound. Both use real transport paths;
there is no production mock mode. The credential callback runs only after
packet, access, disclosure and budget admission. Run `runJudgement` with the
assigned Pi session under the worker's existing lease keeper. It records raw
responses, normalized assessments and policy decisions separately. It proposes
further work; it does not write memories, complete obligations or waive checks.
The owning processes connect these proposals to their effects in WP09–WP12.

With a Jev adapter configured, the default is shadow mode: Jev's answer is
retained and compared, while the conventional model supplies the selected
assessment. Reference-only mode is also available. Qualified mode requires an
operator-supplied qualification for every selected family, definition revision,
model release and scope, including an accessible evaluation artifact and a
family-specific selected-option probability threshold. No real qualifications
ship with the catalogue. Choice admission uses the selected option's probability,
not distribution-sharpness confidence. Noul and Score are validated and retained;
automated qualification for them needs a separate evaluated policy and is not
enabled. Missing evidence produces a retrieval disposition; invalid or unavailable
service results use the permitted reference route or remain unresolved.

Each HTTP attempt reserves root resources independently. Only HTTP 429 and 529
retry automatically, up to three attempts per provider and within the deadline.
Saved replies can finish publication after interruption without another model
call. An interrupted request with no saved reply remains unavailable with unknown
usage. Token observations are retained, but unknown cost keeps the maximum
reservation unresolved; it is never reported as a free request. Supply an
appropriate cost maximum for paid providers. Conservative payload/output guards
reject requests larger than their token allowance. Oversized responses are
rejected rather than adopted; their full payload cannot be retained past the
artifact bound.

Reuse is explicit through an assessment ID. The host checks the current packet,
versions, definition contents, provider release, scope, disclosure, freshness and
all artifact dependencies. Unpinned aliases do not receive a reusable release.
A policy-only change can reuse an assessment in a newly admitted job/packet and
produce a new decision. Revocation blocks reuse, including restoration of a saved
decision. Physical purge across external providers remains WP13 work.

`createTaskCheck` registers a finite check bound to the task and job. It inherits
scope, disclosure policy and completion requirements, permits investigation only,
and cannot replace established checks. Inspect and retire it through the host;
expiry, cancellation and retirement block subsequent use.

An owning policy can supply `consistencyRules` for combinations of answers that
cannot be used together for the same subject. Rules reference declared question
keys and alternatives. A matching Jev combination routes to the reference model;
a matching reference combination leaves the work unresolved. The decision records
those matches and provider disagreements. Qualification evidence is rechecked
when restoring a saved qualified decision.

## Formation (WP09)

`packages/formation/src/index.ts` exports `runFormation`. Call it under the
assigned worker's lease keeper with a formation brief, an admitted source revision,
a stable window ID, an operation name and the existing WP08 provider configuration.
Each call processes one window. Use another window ID to advance; reuse the same ID
to recover an interrupted call. A different operation explicitly reinterprets a
source under the assigned policy. It does not revise earlier records automatically.

The trusted connector publishes `memory-tool-events/1` through `SourceService`.
Each event's `content` is a `FormationInput`: a typed candidate, its evidence,
objective, conditions, episode, retention choice, uncertainty and coverage.
Actor identity and explicit selection come from the connector or contribution UI.
This boundary accepts structured captures; it does not treat arbitrary transcript
text as trusted metadata. `based_on` and `corrects` name earlier event IDs in the
same source and policy operation, including earlier windows.

The host reads an admitted snapshot of at most 1 MiB. Windows contain at most
32 events and produce at most 64 KiB of JSON. They stop at an explicit episode
change or the first event after the evidence cutoff. The Pi worker asks J01/J02
for selected candidates, adding J03 for ambiguous boundaries, J04 for methods,
J05 for intentions and J27 for explicit contributions. Independent questions share
a packet. The host checks that the assessment used the admitted candidate and
source evidence before applying the formation retention rules.

Formation writes candidate episode, knowledge, procedure and intention records.
It keeps origin and evidential status separate and preserves the assigned scope.
Explicit user claims also retain their actor scope when the service has a broader
grant; a personal preference does not become a shared rule.
Corrections create candidate challenge links to earlier versions. Episode summaries
can derive from retained contributions without erasing their individual statuses.
A selected hypothetical remains historical simulation or assumption; derivations
cannot turn simulated experience into observations. Intention execution and
correction-driven retirement remain WP12 responsibilities.

The database commits records, links, duplicate receipts, cursor and change event
in one transaction. Event IDs identify observations across source revisions;
similar text in different events remains distinct. A repeated deferred event keeps
its unresolved reason. Pi saves packet and decision IDs so lost responses do not
repeat successful model calls. Window admission is retained before artifact
publication so publication can resume after interruption.

Results include records, duplicate references, deferrals, coverage, inspected
locators, unresolved required material and common source groups. `judgement_usage`
retains each assessment's reported tokens and cost in microunits, including shadow
assessments. Missing monetary telemetry stays unknown. Native locators are kept on
records and reported as unexamined by formation; a text-only import does not claim
to see an image, model or audio file.

`services/harbor-bridge/trajectory_import.py` provides `import_atif`, using the
installed Harbor trajectory models for declared ATIF versions. The caller supplies
a stable source identity, objective and import time, and may explicitly identify
an actor or synthetic experience. Recorded calls/results remain separate from
narrated statements. Missing timestamps use the declared import time with an
uncertainty marker. Missing telemetry remains null. Media paths remain references;
the importer neither fetches nor interprets them. Ingest its returned event document
through the same tool-event adapter. Nested subagent trajectories require separate
imports with their own source identities.

Verification uses authored captures in `evals/formation/capture.json` and scripted
provider responses through real Rust HTTP and Pi sessions. These checks establish
retention and recovery behaviour. Live semantic quality and held-out continuation
benefits remain qualification work; no service key is required for these tests.

## Activation and search (WP10)

An activation assignment calls `runActivation` from `packages/activation`. Supply
an operation ID and an `ActivationQuery` containing the question, task evidence,
scope, optional identities/time/vector filters, existing memory references and
explicit scan/candidate/traversal/context limits. The worker runs under the same
lease keeper as formation and judgement. A returned cursor continues the same
query on a new operation ID; candidates must be deduplicated by memory/version
when combining pages. Identity matches can recur on subsequent scan pages.

The Host's `activate` command creates a scoped, immutable `ActivationWindow` and
publishes it as a job artifact. Its authoritative receipt lives in the database;
a worker-published artifact cannot impersonate a retrieved window. `select_activation`
checks WP08 decisions against that window and the canonical question definitions,
then returns a `ContextPackage`. Repeating the operation reuses recorded decisions
while rechecking current access and availability. Changed or retired candidates
require a new query rather than exposing an old package.

Retrieval uses these paths:

- Explicit record IDs and authoritative entity IDs/aliases. Identity lookup runs
  independently of the text/vector scan page. `Store.find_entities` and entity
  activation queries also provide the reusable entity-centred view.
- [SQLite FTS5](https://www.sqlite.org/fts5.html) or
  [PostgreSQL text search](https://www.postgresql.org/docs/current/textsearch-controls.html).
  Terms are escaped and bound; only permitted
  version IDs reach scoring. Ranking counts matching query terms without global
  corpus statistics, so another tenant cannot affect a record's score.
- Exact cosine scoring over the permitted scan page, when the caller supplies a
  query embedding. The stored embedding must match model, revision, dimension and
  representation. Other channels use reciprocal-rank fusion; explicit identity
  matches take precedence. Memory identity breaks ties.

The scan and identity lookups each inspect at most 500 records per page, with at
most 32 initial candidates. Graph traversal follows support, conflict, derivation,
dependency and counterexample links under its configured bound. Related records
may cross the initial family/freshness shortlist but must satisfy scope, time and
availability checks. Missing or stale endpoints make a group incomplete. Groups
are selected or deferred together; their exceptions are not dropped to fit a byte
allowance. Coverage reports exhausted bounds and missing current embeddings.

Migration 007 updates lexical/entity projections and emits `search_updates` in
record transactions, including backfill for existing records. The reported lexical
watermark is therefore the query's domain snapshot. Read-your-writes uses this
direct scoped path; there is no asynchronous lexical lag. Vector projections may
lag, but stale revisions are excluded and current lexical/entity lookup remains
available. Current scope and availability override a historical snapshot; explicit
recorded-time queries can inspect prior revisions only while their record remains
available.

An authorised administrator can call `embedding_inputs` to read bounded scoped
index updates, pass the supplied text to its embedding provider, and call
`save_embedding` with the same memory revision. Resume after the last returned
`SearchCursor`. A superseded revision cannot accept a late embedding. The selected
representation, `memory-content-v1`, is the record label plus the complete JSON
memory content. This implementation stores one embedding per record version;
changing its model replaces that projection. The local Nomic adapter below generates
these vectors. The database contract tests still use explicitly authored vectors;
the optional model test exercises real inference. No ANN extension or paid
embedding deployment is qualified by these tests.

#### Local embeddings

`packages/activation/src/embeddings.ts` runs
[Nomic Embed Text v1.5](https://huggingface.co/nomic-ai/nomic-embed-text-v1.5)
through Transformers.js 4.3.0 on CPU, using its q8 ONNX weights. It follows the
model's retrieval prefixes (`search_document` and `search_query`), mean pooling,
layer normalisation and L2 normalisation. Vectors retain all 768 dimensions.
The model release and encoder settings are part of the embedding identity, so
queries cannot silently mix with a different stored projection.

Download and check the model once, then use the cache offline:

```sh
npm run embeddings -- download
npm run embeddings -- query "How do I isolate the pump?"
```

The default cache is `.runtime/embeddings`; set `MEMORY_EMBEDDING_CACHE` to use
another directory. Only `download` enables network access for model loading.
Inference runs locally and sends no memory text to Hugging Face. Normal checks
do not download or run the model.

To index retained records, set `MEMORY_HOST_URL` and `MEMORY_HOST_TOKEN` to an
administrator credential for the intended scope, then run:

```sh
npm run embeddings -- index
# Pass the returned `after` object as JSON to process the next page:
npm run embeddings -- index '{"sequence":42,"version_id":"<returned UUID>"}'
```

Each invocation handles at most 20 records and reports `indexed`, `skipped`,
`after` and `exhausted`. Stop when `exhausted` is true. Keep a cursor with its
credential scope and model configuration; restart without a cursor to rebuild
after changing either. `indexEmbeddingPage` exposes the same operation to callers
that persist their own cursor. Failed saves stop the page without returning a new
cursor; retrying the page replaces any vectors already saved. The Host rejects
stale revisions.

The adapter counts the complete prefixed input before inference. More than 8,192
tokens produces an explicit error, never silent truncation. Indexing reports such
records as skipped and advances; they remain available through lexical/entity
retrieval. Shorten or split the record before retrying it. Oversized queries fail
the activation call. Chunked embeddings are not implemented.

For automatic query embeddings, load one `NomicEmbeddings` instance and pass it as
`options.embeddings` to `runActivation`. It embeds the question only; task context
still goes to the semantic judges. Explicit query vectors take precedence. Pi
persists the generated vector with the activation operation, and continuation
pages reuse the cursor's vector. Dispose the model when the worker shuts down.
Cancellation is checked before and after inference; it does not interrupt an ONNX
kernel already running. Indexing is a separate administrator operation, so queries
can encounter records that have not yet been embedded; coverage reports that gap.

The optional local test uses the real Host, SQLite and cached model:

```sh
MEMORY_TEST_EMBEDDINGS=1 cargo test -p memory-host --test activation
```

It indexes three records across two pages, retrieves two paraphrased procedures
with no keyword matches, runs a document beyond 2,048 tokens and rejects input
over 8,192 tokens. The recorded [smoke result](../../evals/activation/nomic-smoke.json)
is functional evidence from two authored cases, not a held-out quality benchmark.

#### Semantic selection and workspace delivery

Semantic selection uses J06 for evidence roles, J07 for method relevance and a
separate J08 question for each prerequisite/exclusion. Advisory instructions are
included in full. Executable definitions up to 16 KiB are read as text for
inspection, without execution; larger/non-text definitions remain unresolved.
Unknown prerequisites produce `needs_investigation`; established unmet conditions
produce `inapplicable`. Satisfied prerequisites do not promote an unqualified
procedure: it remains a `candidate_method`. A method with no established relevance
cannot enter merely because its prerequisites are unknown.

Call `applyActivation(workspace, package)` and checkpoint the returned workspace
with the WP06 controller. `activationAccess(baseAccess, package)` grants only the
selected memory versions and requires the base scope grant to remain valid; it
adds no source or executable-artifact access. Context groups preserve complete
support bundles through rendering. Actual conflicts remain separate conflict
records. An existing-context claim must match entries present in that workspace
before a partial group can be added. The renderer still decides what physically
fits and records its manifest. Intention records are context candidates only;
atomic firing remains WP12.

Validation uses the same SQLite/PostgreSQL retrieval suite and real Rust HTTP/Pi
activation test. The latter covers full definitions, prerequisite states, empty
additions, budgets, decision reuse and stale-package rejection. The authored
`evals/activation/next-step-probes.json` compares three scripted continuations with
an empty-memory baseline. These prove the handoff changes the next-step policy;
they are not held-out live model usefulness or semantic qualification results.


## Consolidation and qualification (WP11)

`packages/consolidation` supplies two operations for an assigned Pi worker. The
caller keeps the existing job lease alive and supplies its scoped synthesis or
execution harness. Neither operation creates a separate model driver.

Call `runConsolidation(host, assignment, session, {id, selection}, options, context)`
with a `consolidation` brief. The selection names up to sixteen assigned memory
references and describes their purpose, mechanism, outcomes and source context.
The Host loads current records, adds linked exceptions within the same bound and
records the evidence cutoff. It follows derivation links to source IDs and groups
accounts that share a source. Unknown lineage and incomplete coverage defer
retention rather than counting as independent support.

The `options.synthesize(window, context)` callback returns a conditional proposal:
a concise summary, common mechanism, variations, untested cases and supported
clauses. An optional procedure contains either advisory steps or an executable
artifact and entrypoint. Both forms carry conditions, checks, stopping criteria,
effects and a replay contract. The Host rejects proposals that broaden any input
scope. J11 checks comparability, duplication and counterexamples; J12 checks the
whole proposal and every clause for preserved conditions, uncertainty and scope.

Successful synthesis creates a candidate, with derivation and challenge links.
It does not qualify the method. The review artifact retains the full proposal,
source groups and cohort; its ID is recorded in the candidate's policy decision.
Pi stores the proposal and judgement IDs so resuming does not repeat completed
synthesis or judging. Current input revisions and policy are checked again at
commit. Maintenance of candidates after later source changes belongs to WP12.

Set `MemoryPolicy.consolidation` to supply the minimum independent source and
held-out group counts, minimum success rate, permitted regression, maximum reported
evaluation cost in microunits, and evaluator profile. Without these rules, automatic
retention and qualification are deferred. A qualification suite must be an original
input artifact of the parent brief. Its author assigns all variants of a source to
one group; the Host rejects groups shared with the training cohort. The configured
evaluator profile must match the assignment.

`start_qualification` creates an `evaluation` child job for a retained procedure.
The Host publishes its input manifest and delegates the suite and any executable
artifact. Jobs can delegate their own published artifacts as well as original
inputs; they cannot delegate arbitrary readable artifacts. Scope, capabilities and
the root budget remain constrained by the parent.

In that child, call `runQualification(host, assignment, session, execute, context)`.
The executor receives each task with one representation: original episodes, concise
summary or candidate procedure. Expected answers and source-group labels are omitted
from that callback input. This is an application boundary, not isolation from a
trusted evaluator that can read the assigned suite. The executor returns the actual
answer, trajectory, known cost and recognition observations. Each trial is published
as an artifact, and completed trials are retained in Pi. Stable operation IDs let
the existing effect machinery reconcile an interrupted execution.

For an executable Python method, `executeProcedure` loads its assigned artifact and
runs it through the existing `InterpreterClient`, with capability checks and the
`reconcile` replay class. Production callers supply the Harbor/ASP transport. The
local integration test uses the same guarded interpreter API with a task-owned
Python process. The helper supports Python function entrypoints with named inputs
and JSON output, with a 16 KiB code limit. It does not implement other runtimes.

`runQualification` returns a WorkResult that links the comparison report in
`child_outputs`. Stop interpreter resources before sending `complete`; the Host
publishes the final WorkResult envelope. Then the parent calls `adopt_procedure`.
The Host requires one observed trial per case and comparison arm, verifies the
report against its trial artifacts, compares answers with the supplied expected
JSON values, and applies the configured success, regression and cost rules. Missing
or incomplete results cannot qualify a method. This first evaluator supports
structured answer comparisons; a different scoring method needs its own contract.

Adoption revises the candidate to `evaluated` and records the evaluation artifact,
profile and tested contexts. Those contexts also become applicability requirements
for WP10 activation. Failed comparisons preserve the candidate and return reasons.
Recognition candidates identify a question definition, model release and policy.
The report records packet validity, answer quality, routing and false negatives;
question, model and routing-policy adoption are returned separately. This does not
change the global judgement catalogue or qualify a Jev family.

The authored transfer fixture exercises both method forms with real Host commands,
Pi persistence and Python execution. It tests minority exceptions, shared sources,
scope expansion, unsupported generalisation, held-out leakage, resume, source
corrections and adoption/deferral. Its recognition observations and semantic judges
are scripted. Reported trial costs come from the configured executor; the existing
root budget independently enforces admitted resource use. These checks establish
functional behaviour, not live semantic quality or longitudinal savings. No paid services are needed.

Run `cargo test -p memory-host --test consolidation` for the SQLite integration,
or `npm run test:store` for database parity. Set `MEMORY_CONSOLIDATION_REPORT` to
write its transfer summary; the saved fixture result is
[`evals/consolidation/transfer-smoke.json`](../../evals/consolidation/transfer-smoke.json).

### Live consolidation diagnostic

The opt-in WP11 test uses Azure `gpt-4.1-mini` to synthesise an advisory method,
then compares the episodes, model-generated concise summary and method on the
same two held-out tasks. Azure judges J11/J12; Jev runs in shadow mode. An
unsupported universal/causal proposal provides a negative control. Model failures
and deferrals are saved without repairing the candidate or overriding adoption.
This diagnostic does not evaluate generated executable code or recognition quality.

```sh
MEMORY_LIVE_CONSOLIDATION=1 \
MEMORY_CONSOLIDATION_LIVE_REPORT=evals/consolidation/live-<run>.json \
cargo test -p memory-host --test consolidation live_consolidation_evaluation -- --ignored
```

The test reads the existing Azure/Jev credentials from `.env` only after explicit
opt-in. Use an unused report path. It allows at most 24 provider calls, with no
provider retries and a US$1 root budget. Each call reserves a conservative $0.025;
unknown bills keep that reservation. Transfer policy uses those reserved costs.
The report separately estimates cost from token counts and public price proxies,
which may differ from Azure contract prices and the Jev alias's actual invoice.
Ordinary test runs skip this diagnostic.

## Maintenance and intentions (WP12)

`runMaintenance` in `packages/maintenance` takes an assigned Maintenance job and a
bounded request: the current record, a proposed revision or removed source,
optional indirect candidates, and a reason. The Host captures the evidence before
judgement. J13 distinguishes wording, correction and a new valid-time state. J15
reassesses remaining support, J16 examines possible indirect dependencies, and J14
checks conflicting claims. Independent questions run in groups of four. Each
packet and decision is retained through the existing Pi judgement machinery.

The Host checks those decisions against its captured evidence and commits only
while the worker still owns the job. Changed records or relations invalidate the
review. A correction revises the record. A world change closes the old valid
interval and creates a successor with a `supersedes` relation. Unresolved conflicts
retain both positions. Wording changes preserve time, evidence and qualification.

Source removal requires an actually revoked snapshot in the record's lineage.
Governed derivatives leave routine use. A separately supported claim can remain
available when the remaining evidence is adequate. Unsupported guidance is retired,
with a pending revalidation intention that links its basis. That intention needs
an execution plan before it can arm; the system does not invent a new budget or
capability grant. Retirement preserves history. Cross-store erasure remains WP13.

Assigned workers poll `memory_changes` before rendering model context. Unfinished
entries that depend on a substantive change become `needs_revalidation`; completed
and unrelated work stays intact. Original citations remain recorded. If event
history is missing, `current_memories` checks the referenced records directly.
Refreshed workspace state and its cursor are stored in Pi under the originating
checkpoint. Refreshing does not append conversation entries during a provider
step, which would otherwise cause Pi to repeat that step.

### Intention definitions and occurrences

An intention record can carry an `IntentionPlan`. It specifies the trigger,
execution brief, readiness evidence, completion rule and optional recurrence.
A finite expiry is required for executable plans. Worker-created plans must stay
within the worker's existing scope, capabilities, inputs, profile and budget.
Definitions without a plan remain pending and can still express unfinished work.

Each occurrence retains its definition revision and progresses through `pending`,
`armed`, `fired` and `completed`. Cancellation or expiry can end any nonterminal
state. An amendment replaces a pending or armed occurrence. A fired occurrence must
be cancelled before its definition is changed; its original job and evidence remain
inspectable. Renewal or follow-up is a new definition revision or a linked intention.

Triggers support a due time, a named resource event, a new source revision, a
completed job result, or an explicitly configured semantic question. Semantic
questions must match a definition in `MemoryPolicy.semantic_triggers`; they do not
become authorised simply because a model proposed them. Activation workers use
`runIntentionCheck` for semantic event matching. Maintenance workers use the same
entry point for J08 readiness and J17 completion checks.

The standalone Host sweeps up to 100 occurrences each second for each configured,
unexpired administrator scope. Embedded hosts call `sweep_intentions` on their own
polling cadence. The database retains due times and event cursors across restarts.
The sweep rotates through pending work, checks setup-time events, and atomically
creates one job when an occurrence fires. Duplicate deliveries return the same job.
A blocked trigger remains visible with an unresolved reason and does not prevent
other occurrences from being examined. Retained source/result events can recover
a cursor gap; otherwise the occurrence explicitly requests current-state investigation.

Completion requires a complete WorkResult, the declared scope and inputs, a matching
JSON result predicate, any required successful effect receipts, and satisfied
semantic conditions. `required_effects` names invocation IDs within the execution
job; the Host adds that job's operation prefix when looking up receipts. Outstanding effects in the execution or its children prevent
completion. `confirm_intention` records the designated person's confirmation;
silence does not count. Cancellation requests stop for the job and its children,
but reports unresolved in-flight effects rather than claiming they were undone.

Database transactions decide competing terminal transitions. By default, checked
completion must commit before expiry. An optional delivery grace of at most one
day accepts only a Host-recorded job completion from before expiry. Late evidence
is attached without reopening a terminal occurrence. Recurrence supports fixed
intervals for time triggers, within a finite definition horizon. Missed windows
expire and the next occurrence uses the current window, without creating a backlog
of executions. Calendar schedules are not implemented in this increment.

### Verification

`cargo test -p memory-store --test maintenance` exercises time/history, source loss,
conflicts and occurrence transitions. `cargo test -p memory-host --test maintenance`
runs the real HTTP Host and Pi worker with scripted semantic answers.
`npm run test:store` runs both paths against PostgreSQL as well as SQLite.
The fixtures cover duplicate firing, transaction rollback, confirmation, unknown
effects, cancellation/completion races, expiry, late evidence and recurrence after
downtime. Workspace tests cover selective invalidation and event-history gaps.
These tests establish functional behaviour. Live J08/J13–J17 quality and long-running
operational qualification remain unverified; no Jev family is promoted by WP12.

## Retention, erasure and administration (WP13)

Deletion is an administrator operation. `begin_deletion` accepts a caller-owned
request ID and up to 100 memory, artifact, job or source identities. Source deletion
covers all retained revisions. A source-version scope identity can also be supplied;
if the same source has other retained revisions, they are included in the plan.
The administrator must cover every affected resource's scope. The current planner
is bounded to 10,000 rows per tenant table and fails before committing if that
limit is exceeded. It is intended for the experimental store, not bulk tenant removal.

The Host commits a revocation epoch, resource barriers, affected session identities
and a durable deletion report in one transaction. It also expires affected owners'
leases and cancels their jobs. Central scope predicates deny subsequent reads,
retrieval, historical access and writes. Worker requests and responses register
resource exposure under the same database write lock. Thus a late response either
registers its exposure before revocation or fails before returning it. The final
provider payload hook checks ownership again after rendering and reservation.
A request already dispatched to a provider remains an external retention obligation.
Workers detect lost ownership through their next Host call or lease heartbeat.

The planner follows retained derivation, source locators, artifact dependencies,
job inputs/results and observed worker exposure. If one job taints a shared Pi
session, all jobs in that container are included. A `supports` relation alone does
not delete a separately retained claim. The report lists surviving claims in
`support_review` so maintenance can reassess them. This is not a new semantic
judgment that their remaining evidence is adequate.

### Administration sequence

1. Call `begin_deletion`, then inspect its report. Retain the registry outside
   the backups that might later be restored. `list_deletions` enumerates reports;
   `inspect_deletion` returns a current report. Retrying the same request ID and
   targets returns the existing report.
2. Stop affected workers and in-flight uploads. Fence and tear down each affected
   Harbor allocation using the existing bridge lifecycle. Acknowledge the sandbox
   boundary only after teardown is verified, or after verifying that the job had
   no allocation. No process is silently assumed stopped.
3. Under exclusive ownership, call `purgeLocalSession(directory, id)` or
   `purgePostgresSession(pool, tenantId, id)`. The local adapter uses Pi's supported
   repository deletion API and verifies that SQLite, WAL and SHM files are absent.
   It removes the worker manifest and retains a minimal deletion marker and lock
   file. PostgreSQL deletion locks the lease, requires the Host's deletion registry
   entry, then deletes the session and its cascading item/value/list rows.
4. Use `acknowledge_purge` for the verified session, sandbox and upload boundaries.
   An acknowledgement is an administrator assertion about an observed operation.
   It does not make an external deletion request. Active or unacknowledged
   containers prevent `purge_deletion` from proceeding.
5. Call `purge_deletion`. Artifact objects are removed before SQL metadata so an
   interrupted attempt can retry. Memory versions, relation payloads, source
   locators, lexical/vector indexes, affected cached windows, assessments,
   maintenance/formation results and intention copies are removed. Old job and
   effect content is scrubbed. Minimal identifiers, billing state, effect outcomes
   and denied request identities remain. An old request cannot become new work.
6. Inspect `live_payloads_removed` separately from the remaining obligations.
   Provider retention, backups and database storage reclamation remain pending
   until independently verified and acknowledged. SQLite uses secure deletion,
   but WAL, filesystem snapshots and PostgreSQL vacuum/backup retention still need
   administration. There is deliberately no single “everything erased” flag.

Expired job history now enters this deletion workflow through `prune_history`.
It no longer removes the only links to session or artifact copies before those
copies have been handled. Its count is the number of newly denied jobs; use
`list_deletions` to see their cleanup obligations.

### Clean continuation and effects

`continue_deleted_work` takes a deletion ID, old job ID and a fresh `SubmitJob`
command. Supply only surviving goals, obligations and authorized evidence. Normal
admission rechecks all input references and the policy/profile. It allocates a new
session and operation; it never copies the old transcript, operation state, custom
values or tool commands. Other submissions for the same task also avoid deleted
session identities. Existing profile changes already allocate an independent session.

An in-progress effect becomes `outcome_unknown` when ownership is revoked. The
report retains its identity and state without its arguments or generated receipt.
A continuation with tools stays blocked until those outcomes are reconciled.
`reconcile_deleted_effect` accepts a terminal observation and a separately governed
evidence artifact. “Not performed” closes the old invocation; it never prepares it
for replay. Work without tools can continue while an external outcome is unresolved.

### Restore and upgrades

Restore into an offline environment. Set the Host configuration's `restore_registry`
to a JSON array of current deletion reports. Startup applies every barrier before
binding the HTTP listener or starting timers, and requires an administrator covering
each report. It resets cleanup acknowledgements because restored copies need another
purge. Apply `applyLocalDeletionRegistry` to restored local session directories
before admitting workers. PostgreSQL sessions check `deleted_sessions` even if a
stale backup restored a formerly valid lease. Backups without the separately retained
registry must not be served. This is a restore procedure, not automatic detection
that an arbitrary database file came from an old backup.

Domain schema migration 10 installs the deletion registry and exposure tables;
existing migration tests cover upgrades and reject newer schemas. Pi storage and
worker family checks remain strict. The pinned backend has no general historical
rewrite or cross-version migration facility. For an incompatible profile/session,
review surviving inputs offline and create a fresh assignment with the supported
profile. Do not edit a worker manifest to bypass compatibility checks or reopen a
deleted session under its old identity.

### Retention boundaries

The policy owner must set retention purpose and expiry when granting work or
retaining a record. The current storage boundaries are:

| Class | Purpose and expiry | Erasure boundary | Backup treatment |
| --- | --- | --- | --- |
| Transient stream fragments | Live delivery; discard after delivery | Worker/process lifetime; persisted fragments belong to the session | Do not back up separately |
| Workspace scratch | Active reasoning; assignment lifetime | Session values and sandbox teardown | Govern with the containing session |
| Resumable session content | Resume authorized work until job retention expires | Entire Pi container | Expiry or deletion remains a reported obligation |
| Retained memory | Policy purpose and record decision expiry/review | Versions, relations, indexes and governed derivatives | Apply the deletion registry before restore |
| Source artifacts | Authorized evidence use under the source policy | Snapshots, object copies and locators | Independently held upstream sources require their owner's action |
| Evaluation fixtures | Explicit evaluation purpose and approved fixture lifetime | Fixture-owning workspace/container | Never infer permission to retain restricted samples |
| Minimal operation receipts | Prevent replay and reconcile billing/effects | Content is scrubbed; only permitted identities/outcomes remain | Retain the deletion registry independently |

Backup lifetime is an operator policy, not inferred from a successful SQL delete.
The runtime leaves the backup obligation pending until expiry or removal is verified.
It cannot erase exports or copies outside the registered storage boundaries.
`retention_class` and the brief's `retention_policy` remain policy references;
this increment does not invent a universal TTL for every retained memory.

For `disclosure_policy: "restricted"`, the Pi worker denies provider calls unless
its trusted profile lists the provider in `restrictedProviders`. The Host applies
the same requirement to judgments using `restricted_providers` in its private
configuration. These lists mean the operator has reviewed zero-content-retention
eligibility; the code does not infer it from a provider name. Empty lists deny such
work. Provider acknowledgements in deletion reports remain separate from eligibility.

### Verification

The retention suites cover SQLite and PostgreSQL revocation, derivatives versus
support, source deletion, artifact byte removal, expired-request replay denial,
uncertain effects, authorized continuation and actual pre-deletion backup restore.
The Host fixture verifies role restrictions and a resource encountered after the
initial brief. Pi tests cover exclusive deletion, WAL/SHM cleanup, restored session
rejection and a real tool effect that does not execute again in the fresh session.
A worker test revokes access between reservation and final payload dispatch and
observes zero provider calls. All provider responses in these tests are scripted;
no paid service or external provider deletion is claimed.

## User and operator API/CLI (WP14)

`POST /v1/commands` now accepts `user` requests. `UserRequest` and `UserMutation`
are part of the same Rust-generated schemas used by the CLI and Pi worker. Client
credentials can inspect and contribute within their assigned scope. Policy creation,
policy revision and dispatch controls require an administrator; tenant-wide dispatch
switches additionally require an unrestricted tenant scope. A worker can prepare a
decision only for its own job with a current fence.

User writes and their request receipts share a transaction. The Host supplies the
actor identity. Corrections use expected revisions, preserve scope and family, and
publish a `memory_maintained` notice for WP12 workspace refresh. Demonstrations and
preferences use ordinary episode/knowledge records with attributed status and
candidate qualification. Explicit assumptions and simulations retain that status.
Human input does not substitute for procedure qualification.

Temporary explorations have a purpose, scope, finite expiry and at most 16 records
within 512 KiB. They are outside the collection/search index. Promoting a selected
record is explicit, revision checked and retry safe; its assumption status remains.
Expiry denies further reads/writes. Physical removal uses the WP13 deletion flow,
which also revokes and removes dependent exploration, decision and render-context
copies. Automatic expiry cleanup is not a new service in this package.

Prepared decisions identify the affected job, owner, missing information/authority,
deadline and fallback. Approval is accepted only from that owner before the deadline
and never expands the brief's authority. Start, resource reservation, final provider
egress and effect dispatch reject pending/declined requests or disabled profiles.
Judgement dispatch also checks provider and family switches. These checks admit new
work; they cannot recall a request already dispatched. Cancellation and result/usage
reporting remain available. An expired/declined job can be cancelled and replaced;
there is no implicit approval or automatic launch on answer.

Render manifests are published by the assigned workspace harness at selection,
payload checking and response recording. The Host checks their assignment, operation
and profile and keeps the current status for each decision ID. `inspect_task` returns
three manifests per page, a continuation ID, the actual job/result and explicit
coverage. Empty pages do not imply any model inspection. A recorded selection is not
a claim that the model relied on every selected item.

Notifications are a scoped outbox pull with actor preferences (`material`, `blockers`,
`completion`, `muted`). Material delivery excludes routine leasing/reservation noise.
Consumers acknowledge each event by ID after handling it; acknowledgements suppress
subsequent delivery to that actor. Muting/filtering advances the scan cursor without
changing the job, intention or decision. Repeated unacknowledged polls can repeat an
event, so consumers use event IDs for their own idempotence. `changes` returns the
scoped event page and associated revision notices independently of notification
preferences.

Run `npm run memory -- --help` or follow the [user guide](../user-guide.md). `init`
creates separate private client/admin credentials and loopback SQLite configuration.
`call --file` covers the complete Host contract; named commands cover common reads,
and `mutate --file` sends a user/operator mutation. Reuse an explicit request ID on
ambiguous retries. The example driver saves full requests before calling the CLI.
No model worker, web UI or notification transport is launched by this CLI.

`ingest_source` lets a trusted connector publish one source revision over the same
authenticated endpoint. The caller supplies the `SourceVersion` without a snapshot
artifact and the content: a JSON string is stored as text, any other value as JSON.
The adapter follows the declared kind (document, table, model or tool events) and
validates the content; the service allocates and uploads the snapshot artifact and
registers the version within the caller's scope. Snapshots are limited to 1 MiB.
Client and administrator credentials may publish; worker credentials may not. Content
the adapter cannot parse is reported as an invalid payload, not a host failure.

`npm run live -- DIRECTORY prepare|work|capture|form|show|reset` runs
`examples/live-walkthrough.ts` through `scripts/ts-loader.mjs`, which resolves the
packages' `.js` imports to their TypeScript sources and transpiles them without type
checking. The driver submits an investigation job for the Host's worker pool, waits for
it to settle, converts the published findings into `memory-tool-events/1`, publishes them
with `ingest_source`, submits a formation job over that source and reports what the pool
retained. The findings connector is example code.

## Worker pool, credentials and operations (WP15)

`issue_worker_credential { job_id }` is an administrator command. The Host reads the
job within the administrator's scope, refuses jobs past their deadline, and mints a
32-byte random token bound to `Role::Worker { job_id }` with the brief's scope, a
fresh worker actor ID and the job deadline as expiry. The token is returned once in the
response and otherwise held only in Host memory alongside the configured credentials;
expired issued tokens are pruned on each issuance and at most 10,000 are held. A
restarted Host knows none of them. Configured credentials in `host.json` continue to
work unchanged.

A `pool` credential (`role: {"kind":"pool","classes":[...]}`) may only call
`claim_next { processes, lease_seconds }`. Work classes follow the deployment profile:
`interactive` covers activation and investigation, `deferred` covers formation,
consolidation, maintenance and evaluation; every named process must fall within the
pool's classes. The Host lists the oldest queued jobs for those processes within the
pool's scope, mints a worker credential for the first, claims the job **as that worker
actor** so the lease owner and the credential agree, and returns `work { assignment,
credential }`; a lost race moves to the next candidate and `idle` means nothing is
eligible. The Host timer also runs coordinator `recover` every five seconds for each
administrator scope, so an expired lease returns to the queue without operator action.

With `worker_pool` in `host.json` (`command`, `args`, `working_directory`,
`restart_delay_seconds`), the Host spawns one child per pool credential and passes the
pool identity only through its environment (`MEMORY_HOST_URL`, `MEMORY_POOL_TOKEN`,
`MEMORY_POOL_CLASSES`, `MEMORY_POOL_INDEX`). An exited child is restarted after the
configured delay, doubling per rapid failure up to one minute; shutdown sends SIGTERM and
kills after ten seconds. `npm run memory -- enable-pool DIRECTORY` writes this
configuration and a `worker.json` for an initialized instance.

`packages/supervisor` is the Node child. `main.ts` reads `worker.json` (models,
concurrency, poll and lease intervals, judgement mode and maximum, the `.env` path) and
runs `runSupervisor`, which polls `claim_next` per free slot and dispatches by process.
Investigation attaches the assigned Pi worker with the job credential, constrains the
Azure reply to the `WorkResult` contract through `before_payload`, and delivers granted
input memories through `current_memories`; a granted memory that has since been revised
or withdrawn settles the job as `blocked` with the reason instead of consuming attempts.
Formation runs one `runFormation` window per input source with a window ID derived from
the job and source, so a retried attempt resumes rather than paying again, and completes
the job with the combined coverage; unexamined material is carried as unresolved work, so
such results are `partial`. Each judgement call is logged as an operator trace (provider,
questions, answers, latency, tokens). A failed attempt shortens its lease to one second
so recovery can requeue it. Other process kinds are not claimed by this pool and stay
queued.

`claim_next` orders candidates by the number of leased or running jobs in each project, then
by age, so a busy project cannot starve a quiet one within the pool's scope; the pool keeps
separate interactive and deferred slots (`worker.json` `concurrency`). The formation
operation is the brief's purpose: jobs sharing a purpose continue a source's cursor, a new
purpose reinterprets the source. `GET /healthz`, `/readyz` and `/metrics` serve liveness,
database readiness and Prometheus counts (jobs by state, issued credentials, uptime) without
credentials or identifiers. Administrator `backup { directory }` writes a consistent SQLite
snapshot, artifact copy, `deletions.json` and `manifest.json`; the CLI `restore` command
rebuilds an isolated instance with `restore_registry`. `memory-migrate` copies a snapshot into
an empty PostgreSQL store (§20.4). Procedures are in [docs/runbooks.md](../runbooks.md);
production manifests in `deploy/`. Dispatch for activation, consolidation and maintenance,
credential persistence and a live production run remain open.