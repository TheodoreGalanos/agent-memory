# Agent Memory — complete implementation plan

**Version 1.0 · 17 September 2026**

Extracted from §22 of the end-to-end implementation specification. All sixteen packages form the delivery scope. Tests are delivered with each package; the final package assembles qualification and release evidence.


## Implementation progress

Work proceeds one package at a time. WP14 is limited to API and CLI at Theo’s request; web UI is deferred.

| Package | Current state | Evidence / next action |
| --- | --- | --- |
| WP01 | Complete — contract and compatibility baseline | Rust contracts and generated JSON Schema/TypeScript; scoped C/D fixtures; Pi binding, real SQLite smoke tests, upstream conformance and pinned ASP schema. See [compatibility](compatibility.md). |
| WP02 | Complete for both local database backends | Shared SQLite/PostgreSQL record, migration and artifact suites pass. Cloud artifact storage remains unqualified. |
| WP03 | Implemented and locally verified | Durable jobs/events, authenticated HTTP, fenced attempts, budgets, effects and result publication. See [runtime](runtime.md). |
| WP04 | Implemented and locally verified | Assigned Pi worker, OS-locked SQLite and fenced PostgreSQL; retry/deferred/result recovery and backend conformance pass. Live model qualification is pending. |
| WP05 | Complete for the local Docker and bounded Daytona profiles | Allocation receipts, restart/orphan reconciliation, endpoint renewal, protected images, export publication and verified teardown. See the bridge guide. |
| WP06 | Complete for the scripted text profile | Typed workspace, access-aware rendering, conflict preservation, compaction and SQLite/PostgreSQL restore pass. Live model qualification remains pending. |
| WP07 | Complete for scripted investigations and the local Docker profile | Scoped child jobs, definition expansion, wait/resume, explicit reuse, coverage-aware results and persistent Python checkpoint/recovery pass. |
| WP08 | Complete for the judgement contract and reference/shadow runtime | All 29 families, selected batching, typed providers, policy separation, scoped reuse and temporary checks pass locally. Live smoke revealed criteria ambiguity; semantic qualification remains pending. |
| WP09 | Local implementation verified; live semantic qualification incomplete | SQLite/PostgreSQL capture tests pass. Azure strict schemas and the approved 0.01 sum tolerance pass live checks. The repeat run admitted 20/20 replies per provider, but Azure made four incorrect retentions and missed one expected contribution. Jev matched 20/20 expected decisions in shadow mode. [Live report](../../evals/formation/live/README.md). |
| WP10 | Implemented for bounded SQLite/PostgreSQL retrieval and scripted semantic selection | Scoped entity/text/vector retrieval, current-version checks, grouped context delivery and Host/Pi tests. Live model usefulness and a paid embedding/ANN profile remain unqualified. |
| WP11 | Implemented for bounded consolidation and authored transfer comparisons | Source groups, conditional proposals, procedure manifests, child evaluation and policy adoption. Live synthesis and general transfer quality remain unqualified. |
| WP12 | Implemented for bounded maintenance and scripted intention checks | SQLite/PostgreSQL history, source support, scoped workspace refresh, atomic firing and checked completion. Live semantic quality remains unqualified. |
| WP13 | Implemented for the bounded SQLite/PostgreSQL runtime | Revocation, exposure, cross-store purge, session administration, clean continuation and restore barriers. External provider/backup erasure requires verified acknowledgement. |
| WP14 | Implemented for API and CLI; web UI deferred | Scoped user operations, recorded task context, policy/dispatch controls, pull notifications and a runnable [user guide](../user-guide.md). |
| WP15 | Implemented for the local profile; production profile written, not run | Worker pool with fair claiming and class capacity, runtime credentials, recovery timer, fault-injection tests, backup/restore with deletion registry, health/readiness/metrics, deployment manifests, SQLite→PostgreSQL migration and [runbooks](../runbooks.md). Open: process inputs for activation/consolidation/maintenance dispatch; a live production run. |
| WP16 | Release candidate published; qualification and stories open | [RELEASE.md](../../RELEASE.md): frozen matrix, all 29 families shadow with the catalogue fallback, operational/semantic/behavioural results separated, gates passed; [traceability](traceability.md) resolves WP01–WP15. Open: §21.1 agent story, unattended §21.2, §21.3 as one story, all semantic qualification and the process studies. |

**WP01 checks (17 September 2026):** `npm run check` passed in the working
folder and a separate installation under `/tmp`. Rust formatting, Clippy, upstream
source compilation, generated schemas and TypeScript checking passed. Tests:
10 Rust + 21 TypeScript + 54 upstream in-memory + 105 upstream SQLite = **190 passed**.
The final locked npm install reported zero known vulnerabilities. No live model or
remote sandbox integration is claimed.

### WP01 implementation choices

- The first model profile is the upstream scripted `faux/faux-1` provider, used only in tests. Live model selection and qualification are still pending; no paid calls were made.
- Schemas are generated from Rust. Scope/deadline checks and result completeness are exercised in Rust; TypeScript consumes the generated wire types. Transport clients arrive with the actual WP03 API.
- Harbor is pinned to the reviewed source revision. Its installation, provider images and execution tests belong to WP05. The ASP schema is pinned separately because the RFC links to a different reference branch.
- Only the current contracts and harness binding are implemented. Later packages have no placeholder services or success paths.

### WP02 implementation and evidence

`crates/memory-store` now implements record batches/corrections, temporal queries,
policy revisions, typed relations, entities/aliases, source adapters and artifacts.
The domain types are in `crates/memory-domain/src/records.rs` and `sources.rs`.
[Storage behavior](storage.md) describes the interfaces and limits.

`npm run check` passed: **192 tests passed** (12 Rust, 21 TypeScript, 159 upstream).
Formatting, Clippy, schema generation and compilation passed. The two new Rust
suites cover SQLite repository behavior and migration upgrades. PostgreSQL
counterparts are explicitly ignored in that command; `npm run test:store`
enables them against a disposable local cluster.

**PostgreSQL follow-up:** Theo approved removal of orphan shared-memory segment
65537. It was rechecked with no attached processes before removal. The disposable
PostgreSQL 14.17 cluster then started and passed the shared repository, migration
and coordinator suites. WP02's database parity requirement is now met. No existing
database was used or changed; cloud object storage remains unqualified.

### WP02 implementation steps

1. **Implemented:** Add typed retained records, temporal intervals, relationships, policies, entities and source locators.
2. **Implemented and checked on both databases:** Add transactional migrations and shared SQLite/PostgreSQL repositories. Test scope checks, corrections, relation direction/cycles and stale updates on both databases.
3. **Implemented:** Add governed artifact allocation, streaming publication, bounded reads/export and interrupted-upload recovery.
4. **Implemented:** Add revision-preserving document, structured table/model and tool-event ingestion. Reopening unavailable evidence must report that state.
5. **Passed on SQLite and PostgreSQL:** Run the same fixtures against SQLite and a temporary local PostgreSQL cluster, then run the full project checks and record limitations.

Schema groups are added with the operations that own them. WP02 covers retained
records (including intention definitions), relations, policies/decisions, entities,
sources and artifacts. Jobs, attempts, leases, budgets and delivery state belong to
WP03; judgement, rendering and deletion administration tables arrive with their
respective packages. This avoids unused tables with speculative payloads.

### WP03–WP05 implementation and evidence

1. **WP03 implemented:** add the coordinator migration and atomic job/receipt/event
   operations; fenced claims, renewal and cancellation; timers and recovery;
   parent/child budget reservations; effect reconciliation and retention cursors.
   Expose these through the Axum host and generated request/response contracts.
   Shared database tests exercise duplicate commands, rollback, stale ownership,
   expired leases, unknown effects, cancellation and budget limits. Authentication
   tests check tenant, scope, role, assignment and expiry.
2. **WP04 implemented:** attach one real Pi operation to each assignment; retain
   provider reservations and application results; recover lost completion replies,
   retries and deferred provider handles. Wrap native tools with host effect
   receipts. Local sessions have kernel ownership locks and consistent backups.
   PostgreSQL implements Pi storage/repository contracts with transactional lease
   checks. Its 55 checks cover storage, repository behavior, streaming-fork
   semantics, takeover and a persisted real harness result. A separate integration
   drives a Pi worker through the real Rust HTTP host, including a native read,
   provider accounting and result-artifact publication.
3. **WP05 transport implemented:** validate a trusted ASP allocation descriptor;
   provide remote file I/O and shell execution through bounded OpenSSH helper calls;
   retain execution IDs, enforce endpoint epochs/expiry and confirm process-group
   termination on timeout, cancellation and adapter cleanup. Full retained output
   can be streamed for artifact publication; bash spill paths are readable through
   the remote file interface. The Python Harbor binding uses the pinned public
   lifecycle API and fails explicitly when provisioning or cleanup fails.
4. **WP05 lifecycle implemented:** `Lifecycle` reserves resources and records the
   allocation through coordinator effects. Docker and Daytona adapters find
   allocations by provider labels, recover retained bindings, fence expired work,
   publish declared files through the host, and verify deletion. A private local
   queue retains incomplete exports and cleanup. Worker callbacks renew endpoint
   ownership and finish exports before completing a job. The protected Docker
   image blocks outbound traffic and limits CPU, memory and PIDs. A watchdog stops
   generated processes that escape their original group when the lease expires.
5. **WP05 qualification:** Docker passes the complete Rust host → pinned Harbor →
   Pi native tools over ASP → host artifact → confirmed deletion path. It also
   verifies same-allocation restart, protected control files, unprivileged tools,
   blocked outbound traffic and escaped-child expiry. Daytona uses a bounded
   1 CPU / 1 GiB / 3 GiB profile with a 15-minute TTL. The same complete path
   passed against Daytona under Theo's US$5 test limit. Its egress proxy accepts
   TCP but blocks TLS/data transfer, which the test checks. A direct not-found
   response confirms deletion even when the provider list lags. No live model
   calls were made.

**Observed checks (17 September 2026):**

- Full project checks: formatting, Clippy, builds, generated schemas and TypeScript;
  15 Rust tests, 34 TypeScript tests and 159 upstream tests passed. The Rust result
  test also runs a separate real host/worker integration.
- Disposable PostgreSQL: 6 shared domain-store tests and 55 Pi backend checks passed.
- Real OpenSSH: 2 tests passed, including binary transfer, readable spill output,
  epoch rejection and confirmed cancellation during both execution and cleanup.
  A separate helper regression verifies cancellation after stdout has closed.
- Python follow-up: 16 helper/lifecycle tests passed in the default suite. The
  Harbor environment passes all 20, including four offline Daytona profile and
  lookup tests. The deletion race regression failed before the fix and passed
  afterwards.
- Docker and Daytona: one full integration per provider passed. Both verified
  the same sandbox after bridge restart, Pi tools, lease renewal/expiry, artifact
  bytes and confirmed deletion. No test sandbox remained in the final provider
  listing. Runtime details and the bounded cloud profile are in the bridge guide.

The enabled checks overlap; these figures are per command, not a summed count of
unique cases. PostgreSQL and OpenSSH tests are opt-in and are reported separately
from the default suite. See [runtime](runtime.md) for commands and operational
limits. Axum and `pg` were added with Theo's approval. The later WP05 follow-up installed an isolated Colima/Docker engine and pinned
Harbor with Theo's approval, then used his Daytona test key. Azure model credentials
remain unused.

### WP06 implementation and evidence

1. **Implemented:** Rust workspace and render-manifest schemas with generated
   TypeScript types. Working entries carry origins, evidential status, scope,
   cited versions and child provenance. Scratch lifetime, recovery retention and
   permission to form memories are separate.
2. **Implemented:** Pi checkpoint projectors and a bounded renderer that preserves
   governing context, obligations and opposing claims. Deferred evidence produces
   a narrower next step. Each request records its actual projected contents,
   source coverage, tool schemas, profile and estimated/final size.
3. **Implemented:** current assignment/access checks before rendering and at the
   final payload. Access changes discard opaque history. A provider guard prevents
   Pi's recoverable hook errors from allowing a rejected request to continue.
4. **Implemented:** compaction carries the typed checkpoint and historical custom
   results. Restoration and tree navigation use the destination's working state.
   Task-local dependency checks preserve completed reports and unrelated valid-time
   scopes. Temporary contribution selection remains an input to WP09, not a write.
5. **Verified on 18 September 2026:** `npm run check` passed: 15 Rust, 51 TypeScript,
   159 upstream Pi and 16 Python tests (four optional Daytona tests skipped by the
   system Python). Formatting, Clippy, generation and builds passed.
   `npm run test:store` passed six shared SQLite/PostgreSQL checks and 56 Pi
   PostgreSQL checks, including WP06 compaction and manifest restoration.

Tests demonstrate rejection before provider delivery for oversize required context,
provider-added payload size, access changes during rendering and refused budget
reservations. They also cover conflict preservation, deferred evidence, unknown
cache metrics, selective revalidation, branch navigation and custom-result recovery.
The test transport supplies deterministic responses; no Azure or Daytona calls were
made. Live tokenizer/payload/cache qualification remains pending. Physical erasure,
formation and maintenance scheduling retain their later packages. Child execution
is covered by WP07 below.
See [runtime](runtime.md#workspace-state-and-context) for the API and limitations.

### WP07 implementation and evidence

1. **Implemented:** typed investigation plans expand referenced definitions into
   bounded child briefs. The host checks child scope, tools, input access, depth,
   concurrency and root allowance. Each child has an independent Pi session.
2. **Implemented:** idempotent spawning, dependency waits and parent reopening.
   Completed-result reuse checks the evidence and interpretation basis, current
   access and retention. Root cancellation blocks further spawning and propagates
   through owned children. Migration 004 stores dependency and artifact links.
3. **Implemented:** delegated child findings enter the workspace with their origin.
   Aggregation retains counterexamples, missing coverage, unresolved effects,
   applicability and evidence cutoff. Oversized returns become partial reference
   results; the complete child artifacts remain available.
4. **Implemented:** the Pi `python` tool executes through the existing ASP sandbox.
   Variables persist between bounded calls. Selected JSON objects can be published
   as checkpoints and restored after interpreter loss. Unknown executions require
   receipt inspection and host reconciliation; they are not automatically repeated.
5. **Verified on 18 September 2026:** `npm run check` passed formatting, Clippy,
   schema generation, builds, 15 Rust, 60 TypeScript, 159 upstream Pi and 21 Python
   tests. Four optional Daytona Python tests were skipped by system Python;
   the installed Harbor environment passed all 25 offline bridge tests. The Rust result test
   additionally runs the real host/Pi publication and investigation fixtures.
   `npm run test:store` passed six shared SQLite/PostgreSQL checks and 56 Pi
   PostgreSQL checks. `npm run test:asp` passed two real OpenSSH tests.
   `npm run test:docker` passed the full host → Harbor → Pi Python tool →
   checkpoint/restore → artifact publication → verified deletion path.

The investigation fixture expands a clearance definition, runs two child sessions,
reopens the parent and preserves a child's two-metre counterexample and missing
night-shift evidence despite their omission from the parent's scripted draft.
A cached-receipt revocation regression failed before its fix and passed afterwards
on both databases. Interpreter tests cover timeout, process loss, duplicate/unknown
receipts, output limits, credential exclusion and explicit restoration. Sandbox
compute is reserved once by its allocation; interpreter output is charged separately.

The first implementation accepts a trusted, bounded question batch; deployment
scheduling remains WP15. No dependency was added and no paid service was used.
Live model semantics and WP07's updated Daytona image remain unqualified. The
[runtime guide](runtime.md#scoped-investigations-wp07) describes limits, caller
responsibilities and recovery. Test counts above overlap across commands.

### WP08 implementation and evidence

1. **Implemented:** all J01–J29 have explicit definitions, bounded evidence packet
   builders and authored answer/disposition/fallback fixtures. Questions name their
   evidence fields. Overlapping properties are assessed independently, and selected
   compatible families share one request. TypeSafe's official guidance and cookbooks
   informed the decomposition; details are in [compatibility](compatibility.md).
2. **Implemented:** real Jev HTTP and Pi conventional-model adapters, typed response
   validation, per-attempt reservations, bounded retries and separate raw responses,
   assessments and policy decisions. Shadow is the default Jev mode. Operator
   qualification is specific to family, definition, model release and scope.
3. **Implemented:** host-verified evidence, disclosure and lineage; assessed-result
   reuse with current access/freshness checks; finite investigative task-local
   checks. Migration 005 stores assessments, decisions and local checks. Saved
   replies recover a lost acknowledgement without repeating the provider call.
4. **Verified on 18 September 2026:** `npm run check` passed formatting, Clippy,
   generation, builds, 16 Rust, 105 TypeScript, 159 upstream Pi and 21 Python tests.
   Four optional Python bridge cases remain skipped in the system environment.
   `npm run test:store` passed six SQLite/PostgreSQL store checks, the judgement
   HTTP/Pi integration on PostgreSQL and 56 Pi PostgreSQL checks. The final recovery
   fixture and TypeScript check passed after adding lost-acknowledgement coverage.
5. **Live smoke:** the cookbook model `jev-1.12` was rejected. `jev-latest` returned
   `jev-1.13.0`. Three Choice results agreed between a combined request and two
   separate-family requests, but two differed from authored expectations. The
   successful requests reported 4,012 input and 469 output tokens, approximately
   US$0.00017 at the documented price. The rejected request returned no billing
   information. [Retained report](../../evals/judgement/live-smoke.json).

J01/J02 revision 2 clarifies the ambiguous criteria exposed by that smoke test.
A subsequent three-request live comparison returned all three expected labels in
both the batch and separate-family calls, using the same returned model release.
The [revised report](../../evals/judgement/live-smoke-revised.json) retains the
responses: 4,263 input and 451 output tokens, approximately US$0.00018 at the
documented price. This single authored example is not held-out qualification.
All families remain semantically unqualified, with the reference route available. No production dependency was added and no Azure model or sandbox was used for WP08.

### WP09 implementation and evidence

Formation now accepts bounded, structured captures with explicit source metadata.
The host admits the event window, Pi batches the relevant WP08 checks, and Rust
constructs and commits candidate records. Records, correction/derivation links,
duplicate receipts and cursor advancement share one transaction. The runtime guide
documents the connector contract and one-window execution API.

The authored property-location sequence retains the initial empty instance query,
its qualified inference and the later type-level finding. Additional cases cover
an episode summary, method, explicit user preference, intention proposal, scratch
material, selected simulation and unsupported generalisation. Integration checks
exercise native locators, duplicate revisions, independent same-text events,
lost commit responses, substituted evidence and rollback after a missing correction
target. The ATIF importer separately checks observed versus declared actions,
unknown telemetry and media references with the installed Harbor models.

No production dependency or paid service was used. Native media interpretation,
live model qualification and held-out continuation benefits are not claimed.
Intention execution and correction-driven retirement remain WP12. WP10 activation
is the next package.

**Executed checks:** `npm run check` passed formatting, Clippy, generation/build,
17 Rust tests, 108 TypeScript tests, 159 upstream Pi tests and 21 Python tests.
The broad suite skipped optional PostgreSQL, sandbox, live-provider and Harbor
checks as configured. A subsequent scope regression first failed for a personal
preference under a broad service grant, then passed after narrowing the retained
claim to its actor; Clippy, generation and TypeScript were checked again.
`npm run test:store` passed all six repository/coordinator checks, both Rust HTTP/Pi
integrations on PostgreSQL and 56 Pi PostgreSQL tests. The isolated cluster shut
down successfully. The three ATIF tests passed separately with the installed Harbor
Python environment.

### WP09 live diagnostic

A subsequent authorised live pass used Azure `gpt-4.1-mini` as reference and Jev
in shadow mode on 20 fresh authored cases (22 contributions, 40 provider calls).
The pipeline completed, but only 5/10 expected contributions were retained.
Azure produced seven malformed JSON replies, three answer-contract failures and
one judgement-sensitive missed inference. Jev returned 17/20 admissible replies;
three two-decimal distributions summed to 0.99 and failed the original 0.001 tolerance.
All 17 admitted Jev replies would produce the expected retention disposition,
without establishing full question-label accuracy or qualification.

The [live report](../../evals/formation/live/README.md) retains cases, requests,
responses, actual records, usage and limitations. Estimated token cost was
US$0.026716342 at public proxy prices, not an observed invoice. No incorrect
retentions occurred, but several malformed Azure answers contained unsafe-looking
labels; structural rejection must not be mistaken for demonstrated semantic safety.
Descriptive case IDs were visible, so the result is diagnostic, not blinded accuracy.

Theo subsequently approved a 0.01 probability-total tolerance. Both validators now
accept totals from 0.99 to 1.01 while preserving the returned values and existing
choice/score checks. Boundary regressions pass. Offline revalidation admits all
20 Jev replies, with expected retention decisions in all 20 cases; it makes no
additional paid calls or database writes and does not change the original outcome.

Azure structured outputs are now implemented through Pi's existing request hook.
The same 20 cases were rerun with unchanged question wording: all 20 responses
from each provider passed validation. Azure retained nine expected contributions
and four that should have been deferred (inferred preference, narrated action,
injected evidence and overbroad method), while still missing the cautious inference.
Jev again matched all 20 expected retention decisions in shadow mode. The rerun
used 40 calls and cost an estimated US$0.02789108 at the same proxy prices.

The structural issue is fixed. Before claiming live semantic readiness, review
these semantic failures and the cautious-inference expectation, then evaluate any
prompt/model changes on fresh cases with neutral IDs. Jev remains in shadow mode.

### WP10 implementation and evidence

Activation now runs through Rust contracts, the scoped store and HTTP host, WP08
judgements and a Pi-backed worker. SQLite FTS5/PostgreSQL text search and exact
vector scoring combine with direct entity/record lookup. Identity matches are not
limited to the first ordinary scan page. Scope, source/time constraints and current
availability are checked before exposing candidates; stale embeddings and retained
windows cannot revive revised or retired content.

Support, conflict, dependency and counterexample links form bounded groups.
Incomplete groups remain deferred, and complete groups are admitted together under
the context allowance. J06/J07/J08 assess evidence roles and individual method
conditions. Full advisory instructions and bounded executable definitions reach the
judges and workspace. Missing conditions require inspection; unmet conditions and
withdrawn/unqualified procedures remain explicitly distinguished. No activation
path fires an intention or executes a method.

Migration 007 maintains text/entity projections transactionally and exposes scoped
embedding inputs. Model/revision/dimension/representation checks apply to supplied
vectors. Local Nomic generation uses the approved Transformers.js dependency.
The typed context package
records eligible, retrieved and selected records, coverage, cursor, applicability
and grouped evidence. WP06 context groups keep support bundles whole independently
of actual conflicts. Existing context can yield a deliberate empty addition.

The SQLite/PostgreSQL suite checks scope isolation, vector recall on authored
vectors, stale revisions, retirement, temporal filters, cursors and entity lookup.
The HTTP/Pi test checks semantic decision ownership, full method loading, resume,
empty additions, grouped budgets and the workspace handoff. Three authored scripted
next-step comparisons cover missing, satisfied and unmet prerequisites. These are
functional evidence; held-out live continuation usefulness, production ANN recall
and a paid embedding deployment are not qualified. WP09's live Azure failures remain
open and Jev remains in shadow mode. See [runtime details](runtime.md#activation-and-search-wp10).

**WP10 verification:** `npm run check` passed: 23 Rust tests, 125 TypeScript
tests, 21 Python tests (7 additional optional tests skipped) and 159 upstream Pi
tests. `npm run test:store` passed both database contract suites, all three Host/Pi
integration paths and 56 PostgreSQL session tests; its disposable cluster was
stopped. A separate SQLite migration test confirms existing records are indexed
on upgrade. The broader suite exposed a two-second cold-SDK timeout in the earlier
Azure request-shape test; its deadline is now ten seconds, with assertions unchanged.
No paid services were used.

**Local embedding addition:** Nomic Embed Text v1.5 now generates 768-dimensional
vectors on CPU through Transformers.js 4.3.0, using q8 weights. The adapter indexes
bounded pages through the existing administrator API. Activation can generate a
query vector and reuse it on resume; semantic judges receive the question and task
context without the vector. Inputs over 8,192 tokens are rejected explicitly.
Model download is opt-in, and cached loading is checked with Hub requests disabled.

The full check and SQLite/PostgreSQL suite passed. The optional real-model Host
test indexed three records and found the intended procedure in both authored
paraphrase cases where keyword search found none. It also checked CLI pagination,
input length handling and offline loading. See the [recorded smoke result](../../evals/activation/nomic-smoke.json)
and [run instructions](runtime.md#local-embeddings). This establishes a working
local path; it does not qualify retrieval quality across a larger collection.

### WP11 implementation and evidence

Consolidation now spans Rust contracts, SQLite/PostgreSQL migration 008, Host
commands and Pi operations. Explicit cohorts preserve source groups, minority
exceptions, scope and the evidence cutoff. Conditional proposals pass J11/J12
review before becoming candidates. Both advisory and executable procedures have
manifests and use the existing assignment, artifact and effect boundaries.

Qualification runs as an evaluation child job. It compares original episodes,
concise summaries and proposed procedures on held-out source groups. The Host
checks observed trial artifacts and applies configured success, regression and cost
rules. Successful methods receive evaluated status, evidence/profile references and
tested-context applicability requirements; failed methods remain candidates.
Recognition question, model and routing-policy results remain separate.

The local transfer fixture uses real Host commands, Pi persistence and the guarded
Python interpreter. Both procedure forms solve two of two held-out tasks, matching
the episodes baseline; the concise summary solves one. A deliberately failing
method is deferred. Tests also cover source dependence, preserved exceptions,
scope expansion, unsupported generalisation, held-out leakage, completed-work reuse
and source corrections. Semantic judges and recognition observations are scripted.
No live synthesis, broad model qualification, managed sandbox deployment or
longitudinal cost benefit is claimed. See the [runtime contract](runtime.md#consolidation-and-qualification-wp11)
and [transfer result](../../evals/consolidation/transfer-smoke.json).

**WP11 verification:** `npm run check` passed with 24 Rust tests, 125 TypeScript
tests, 21 Python tests and 159 upstream Pi tests. Optional service/profile tests
remain opt-in. `npm run test:store` passed the SQLite/PostgreSQL repository suites,
all four Host/Pi paths and 56 PostgreSQL session tests on rerun; each disposable
cluster was stopped. The migration-upgrade fixture now removes migrations 007 and
008 when reconstructing its older schema.

An earlier run exposed an intermittent failure in the existing PostgreSQL session
conformance wrapper: concurrent lease grants can reorder fork/create calls before
they reach the repository. A fixture-only attempt to remove that delay exposed a
separate source-snapshot ordering failure. That attempted change was reverted;
PostgreSQL fork concurrency needs a separate investigation. WP11's own database and
transfer tests passed. No production dependency or paid service was added.

**WP11 paid diagnostic:** The [18 September live report](../../evals/consolidation/live-2026-09-18.json)
records 19 calls: Azure synthesis and advisory transfer, Azure reference judging,
and Jev shadow judging. The generated method and faithful summary each solved two
of two held-out cases; episodes solved one. There is no demonstrated advantage of
the method over the summary in this small run.

The negative control failed: Azure approved all six J12 checks on a proposal with
an unsupported universal/causal claim, and the Host retained it as a candidate. It
was not qualified. Jev (`jev-1.13.0`) rejected the bad clause, but also returned
insufficient evidence for comparability of the benign cohort. Neither result
qualifies a provider for general automatic use. The public-price estimate was
US$0.01196, with token usage available for every call; this is not an invoice.
The opt-in harness has a 24-call/US$1 conservative budget and leaves failures visible.
Type checking, focused Clippy, the existing local integration and the paid diagnostic
completed successfully; semantic acceptance of the negative control remains a failed
evaluation outcome. No production policy was promoted.

### WP12 implementation and evidence

Maintenance now captures a bounded evidence review and applies J13–J16 decisions
through the Host. Corrections and world changes preserve their different temporal
meaning. Source loss retires governed derivatives; independently supported claims
can remain available. Open conflicts and revalidation work are recorded explicitly.
Assigned work refreshes its workspace from scoped change notices before rendering.

Intention definitions create durable occurrences. Structured time, source, result
and resource-event triggers use Host rules. A configured semantic trigger uses an
Activation worker. Firing creates one execution job in the occurrence transaction.
Readiness, result conditions, effect receipts, source scope and designated confirmation
are checked before completion. Cancellation, expiry, late evidence, retry and fixed
interval recurrence preserve the occurrence's recorded definition and terminal state.
Worker-created plans cannot enlarge their capability or budget grant.

Primary code is in `crates/memory-store/src/{maintenance,intentions}.rs`,
`crates/memory-host/src/{maintenance,intentions}.rs` and `packages/maintenance`.
Migration 009 stores reviews, occurrences, checks and change notices. The
[runtime guide](runtime.md#maintenance-and-intentions-wp12) describes the APIs,
Host timer cadence, bounds and incomplete-input behaviour.

**Verification (18 September 2026):** `npm run check` passed: 26 Rust tests,
127 TypeScript tests, 28 Python bridge tests, 54 upstream in-memory tests and
105 upstream SQLite tests. `npm run test:store` passed, including the new
SQLite/PostgreSQL maintenance suite, five Host/Pi flows and 56 PostgreSQL session
tests. Focused checks also exercised all supported trigger routes, retry on the
same occurrence, capability escalation rejection and current lease enforcement.

Two regressions found during implementation have coverage: refreshing workspace
state must not cause another Pi provider call; revoking remaining support during
review must reject the maintenance commit. The latter test failed before the fix
and passed after the commit transaction began rechecking source availability.
The regression passed on both database backends. Required effect receipts are
addressed by invocation ID within the execution job, so a definition does not need
to predict a future job's operation ID; a successful-receipt fixture covers this lookup.

No paid services or credentials were used. The semantic answers are scripted;
J08/J13–J17 and custom-trigger quality remain unqualified. Recurrence currently
supports fixed intervals. A missing event history can require explicit investigation,
and newly created revalidation intentions stay pending until they have an executable
plan. WP13 supplies cross-store erasure; user interfaces and production operations remain WP14–15.

### WP13 implementation and evidence

The Host now commits revocation before cleanup, registers authorized worker exposure
and fences affected jobs and sessions. The bounded purge planner covers retained
versions, derivatives, relations, sources, artifact objects, indexes and cached work.
Separately retained supported claims survive and are listed for maintenance review.
Session, sandbox and upload acknowledgements gate live purge. Provider retention,
backups and database storage reclamation remain distinct pending obligations.

Local Pi administration uses the pinned backend's container deletion API and checks
SQLite/WAL/SHM removal. PostgreSQL administration requires the deletion registry and
serializes with session writers. Restore reapplies that registry before Host access.
Clean continuation uses a fresh brief, session and operation; unresolved old effects
must be reconciled before enabling tools. Restricted provider dispatch requires an
explicit operator-approved retention profile. Expired job pruning enters the same
workflow instead of dropping links to unpurged copies.

Focused retention tests cover denial before purge, source and derivative removal,
surviving independent content, late provider dispatch, uncertain effects, restored
backups and a Pi tool effect that does not replay in the clean session. They also
check administrator scope and that a denied read cannot contaminate an unrelated
resource's deletion plan. No paid provider, production backup or cloud deletion was
used. See the [runtime procedure and boundaries](runtime.md#retention-erasure-and-administration-wp13).

**Verification (18 September 2026):** `npm run check` passed: 29 Rust tests,
131 TypeScript tests, 28 bridge tests, 54 upstream in-memory tests and 105 upstream
SQLite tests. The TypeScript run skips 70 opt-in/backend fixtures; the configured
Rust integration tests exercise their applicable HTTP/Pi paths. `npm run test:store`
also passed SQLite/PostgreSQL parity, Host/Pi flows and all 57 PostgreSQL Pi backend
tests. Final Clippy and focused Host/store checks passed after the exposure review;
the interrupted-upload case also passed on both databases. No unresolved failures.

### WP14 implementation and evidence

`POST /v1/commands` accepts `UserRequest`/`UserMutation`, defined in
`crates/memory-domain/src/interaction.rs` and generated into the shared schemas.
`crates/memory-store/src/interaction.rs` and `interaction/mutations.rs` implement
scoped reads, retry-safe mutations keyed by request ID, corrections that retain
history and publish the WP12 change notice, attributed contributions, temporary
explorations with explicit promotion, prepared owner decisions, notification
preferences/acknowledgements and administrator dispatch controls. Migration 011
stores explorations, decision requests, task contexts, notification state and
controls. One execution gate is checked at job start, budget reservation, effect
preparation/dispatch and provider/judgement dispatch: a pending, declined or expired
decision blocks only its job, and disabled provider/family/profile controls reject
new admissions. Assigned Pi workers publish render manifests through `record_context`
and check provider controls at final payload egress; `inspect_task` pages them and
states when nothing was recorded. `packages/cli` and `examples/walkthrough.mjs` drive
the [user guide](../user-guide.md) against a disposable Host.

**Verification (18 September 2026):** `npm run check` passed: 30 Rust tests,
132 TypeScript tests, 28 bridge tests, 54 upstream in-memory tests and 105 upstream
SQLite tests. `npm run test:store` passed, including the interaction suite on both
SQLite and PostgreSQL and all 57 PostgreSQL Pi backend tests. The store suite covers
mutation retries, stale-revision conflicts, scope rejection, exclusion of explorations
from retrieval, owner-only answers, expired requests, muting that does not authorize
work, a second job proceeding while the first awaits its owner, manifest pagination,
acknowledgement suppression, dispatch controls and deletion of exploration copies. The
CLI test initializes an instance, runs every guide step through the real Host, retries
a correction, rejects an operator mutation from the client credential and verifies
persistence across a Host restart.

Two test fixture issues were corrected while assembling this evidence. The assigned
worker fixture used the raw scripted provider, which has no transport payload hook,
so its assertions about `payload_checked` manifests and the provider control check
could never hold; it now routes through the same `onPayload` boundary as Pi's network
providers. The PostgreSQL Pi backend fixture granted leases asynchronously before the
repository's synchronous session-ID reservation, so a concurrent fork/create
conformance case failed in about one run in five; grants are now issued in call
order and the suite passed eight consecutive runs. Neither change touched runtime code.

No paid services, model calls or credentials were used. Web UI, external notification
transport and a worker supervisor are not part of this increment.

**Live use case (18 September 2026):** Theo asked for a runnable guide showing a model,
the judgement providers and memory interacting. Two additions were needed. `ingest_source`
gives trusted connectors an authenticated HTTP path to `SourceService`; until now every
connector, including the ATIF importer's output, could only reach it from Rust in-process.
`examples/live-walkthrough.ts` then drives one investigation job with `gpt-5.4-mini`,
converts its published findings into `memory-tool-events/1`, publishes them, and runs
formation with `gpt-4.1-mini` as reference and `jev-latest` in shadow. Part 2 of the
[user guide](../user-guide.md) documents the run; the transcript is
[evals/live-walkthrough/run-2026-09-18.json](../../evals/live-walkthrough/run-2026-09-18.json).

Building it exposed no runtime defects but several honest constraints the guide now
explains: granted input memories enter the workspace label-only until content is
delivered; the Host refuses `complete` results with unexamined coverage; assessments
older than the brief's evidence cutoff are rejected; judgement reservations without
cost telemetry hold their maximum; and expired leases return to the queue only through
`recover`. Per-job worker credentials initially required a Host restart on this local
profile; the first WP15 slice, `issue_worker_credential`, removed that. `gpt-4.1-mini`
miscopied scope identifiers in two of three attempts, so the task uses `gpt-5.4-mini`
with the `WorkResult` contract as a strict provider output schema. `HostClient` now
reports non-JSON error bodies instead of failing to parse them. Across the day's runs
Jev and Azure agreed on every J01/J02 answer (17 packets). This is a demonstration, not
qualification. Verification: `cargo test -p memory-host --test sources --test
credentials --test authentication`, 25 targeted TypeScript tests (assigned worker, CLI
walkthrough, live driver helpers) and one paid opt-in run with a deliberate Host restart
passed; formatting and Clippy passed. Recorded spend was about US$0.01 per clean run.


### 22.1 Order and parallel work

The table is an implementation order for the full target. Every package produces tests and a usable contract for its dependants. Storage adapters, sandbox work and UI read models can progress in parallel after their contract dependencies are satisfied. Functional, semantic and operational acceptance remain separate.

| Package | Depends on | Complete deliverable |
| --- | --- | --- |
| WP01 Compatibility and contracts | Conceptual source | Pinned upstream manifest, shared schemas, reference-world fixtures and contract harness. |
| WP02 Domain persistence and artifacts | WP01 | SQLite/PostgreSQL repositories, temporal graph, source adapters, artifact lifecycle and migrations. |
| WP03 Coordinator and effect boundary | WP02 | Jobs/outbox, attempts, ownership, budgets, receipts, API auth and recovery. |
| WP04 Pi harness and session backends | WP01–03 | Embedded harness, local Session backend integration, PostgreSQL adapter, hooks and operation recovery. |
| WP05 Harbor and ASP | WP01, WP03 | Provisioning bridge, complete ExecutionEnv adapter, remote cancellation and artifact export. |
| WP06 Workspace and rendering | WP02, WP04 | Typed workspace, bounded context, origin/lineage, compaction and cache-aware layout. |
| WP07 Scoped execution | WP03–06 | Child jobs/sessions, structured inspection, interpreter mode, aggregation and partial results. |
| WP08 Semantic judgement runtime | WP02–03, WP07 | Jev/baseline adapters, all packet schemas, question catalogue, routing and reuse. |
| WP09 Formation | WP02, WP06, WP08 | Incremental capture, contributions, episodes/claims/method/intention candidates and coverage tests. |
| WP10 Activation | WP02, WP06, WP08 | Entity/lexical/vector retrieval, applicability, conflict groups and index consistency. |
| WP11 Consolidation | WP07–10 | Conditional synthesis, executable/advisory methods, reusable checks and qualification. |
| WP12 Maintenance and intentions | WP03, WP08–11 | Revision/support/conflict handling; all occurrence transitions, timers and selective revalidation. |
| WP13 Retention and administration | WP02–06, WP12 | Cross-store revocation/purge, clean-session continuation, backup deletion and admin tests. |
| WP14 User and operator surfaces | WP06, WP09–13 | Complete interaction API/CLI and notification/decision handling; web UI deferred. |
| WP15 Deployment and migration | WP03–07, WP13–14 | Local supervisor, production manifests, tenant fairness, migration and recovery runbooks. |
| WP16 Qualification and release | WP01–15 | Full process/harness studies, all-family Jev reports, end-to-end stories and release evidence. |

### 22.2 WP01 — Compatibility, contracts and fixtures

**Implement:** pin the reviewed Pi source/package set, Harbor and ASP schema/reference revision; compile a minimal harness and selected execution-tool imports; run the upstream conformance entrypoints actually shipped in that revision. Record licence, runtime, SQLite and container/provider prerequisites. Define generated wire schemas, error taxonomy, scope model, work/result contracts and the source-to-requirement matrix. Create the property-location reference world with before/after evidence checkpoints and fake providers.

**Exit evidence:** reproducible clean build; exact API signatures used by the project; baseline fixture validation; a documented list of implemented versus project-owned upstream gaps; no unresolved assumption about raw remote Session or existing PostgreSQL/erasure support. **Tests:** T01, T03 and fixture infrastructure for T15.

### 22.3 WP02 — Domain persistence, sources and artifacts

**Implement:** all logical schema groups required by the domain, repository contracts and both storage backends. Add temporal corrections, typed relations, source locators, entity/alias handling, artifact publish/read/export, migrations and access-aware reads. Provide filesystem/document, structured model/tabular and tool-event/trajectory ingestion adapters.

**Exit evidence:** the same record/temporal/relationship fixtures pass on both databases; artefact publication survives interrupted writes; source reopens preserve revision or report unavailable; policy/state updates reject stale versions. **Tests:** T01, artifact parts of T12 and T14.

### 22.4 WP03 — Coordinator, delivery, budgets and effects

**Implement:** authenticated API, durable jobs/outbox/inbox, task-local and production session ownership, attempt state, retry/wait/cancellation, timer primitives, aggregate reservations and receipt/reconciliation service. Implement narrow worker assignments and event cursors. Establish event and job retention from the outset.

**Exit evidence:** crash tests at every admission/commit/ack boundary; duplicate delivery does not duplicate domain changes; stale owners cannot commit; unknown external outcomes remain explicit; parent/child accounting is correct. **Tests:** T02, early T12 and T14.

### 22.5 WP04 — Pi execution and complete session persistence

**Implement:** worker start/stop, process-local Session attachment, `accept`/`drive` mapping, result replay, hooks, profile/resource loading and native tool wrappers. Integrate Pi's SQLite backend. Build the required PostgreSQL storage/repository adapter with fencing and upstream conformance. Add application entries, snapshot/reconnect and pinned-format migration administration.

**Exit evidence:** a real worker persists and resumes deterministic-provider work on both backends; all used open-operation leaves are covered; lost host/Pi acknowledgements reconcile; model and tool settlement remain distinguishable from application completion. **Tests:** T03, T02 and backend-specific T14.

### 22.6 WP05 — Harbor provisioning and ASP environment

**Implement:** Python bridge, allocation receipts/capability checks, trusted descriptor generation, `AspExecutionEnv` and remote-execution identity/control. Cover every I/O operation used by selected Pi tools. Add sandbox artifact export, endpoint fencing, network configuration and cleanup/orphan reconciliation. Qualify Docker and the selected production provider.

**Exit evidence:** native/local versus ASP tool conformance, binary/path/error parity, bounded large output, verified remote process cancellation and no host fallback; a sandbox loss yields a recoverable or explicitly partial operation. **Tests:** T04, sandbox portions of T02/T12/T14.

### 22.7 WP06 — Workspace, rendering and compaction

**Implement:** typed working state, scratch lifetime, source inventories, origin/provenance, render manifests, conflict-group projection, task-local dependency checks, temporary exploration controls and cache-aware compaction/rebuild. Integrate custom entry projectors and final provider-payload budget checks.

**Exit evidence:** required context survives compaction/restoration; oversize context narrows the next step honestly; actual selected contents are recorded; access changes override cache reuse; completed historical results keep their scope. **Tests:** T05 plus T03 custom-entry recovery.

### 22.8 WP07 — Scoped work and RLM execution

**Implement:** work-brief preparation/expansion, child submission and wait/resume, independent-session permissions, structured operations over referenced artifacts, persistent interpreter mode, coverage-aware aggregation and bounded result publication. Integrate explicit operation caching and cancellation propagation.

**Exit evidence:** a multi-stage investigation runs outside the main context, expands a missing definition, preserves a child counterexample and returns an attributable result; root limits and interpreter-loss recovery hold. **Tests:** T06 with T02/T04/T05 integration.

### 22.9 WP08 — Complete semantic-judgement capability

**Implement:** definitions and packet builders for J01–J29, typed Jev adapter and conventional-model/reference routes, independent batching, retries, validation, policy separation, assessed-result reuse and task-local check lifecycle. Build parameterised fixtures and mock answers for every family before enabling effects.

**Exit evidence:** every family has a complete packet/answer/disposition/fallback contract; invalid service results remain distinct from insufficient evidence; lineage, billing, disclosure and time limits are tested. **Tests:** T11, family fixtures for later T07–T10/T13, and T12 disclosure tests.

### 22.10 WP09 — Formation

**Implement:** windowed/cursor-based capture, event segmentation, explicit user contributions, support checks, family-specific candidate construction, atomic record batches and duplicate/common-source handling. Include multimodal/native-source locators and trajectory imports.

**Exit evidence:** pre-correction and post-correction fixtures retain the correct status; all required meanings survive; scratch/hypothetical material follows its policy; end-to-end formation returns inspected references and coverage. **Tests:** T07, relevant J-family tests and T01 atomicity.

### 22.11 WP10 — Activation and search

**Implement:** exact/entity, lexical and vector adapters; scoped ranking/fusion; support/conflict expansion; full-method shortlisting; semantic applicability; index watermark merging; sufficient empty activation; and context-package delivery. Expose entity-centred views as a reusable query layer.

**Exit evidence:** scoped candidates never leak, conflicts/exceptions remain visible under budget, current writes are retrievable, stale projections cannot resurrect removed content, and held-out next-step probes measure actual usefulness. **Tests:** T08, T05 integration and filtered vector-recall comparisons.

### 22.12 WP11 — Consolidation and reusable methods

**Implement:** cohort selection, source-independence accounting, conditional synthesis, executable/advisory procedure manifests, qualification jobs, adoption policy and candidate recognition criteria. Build the evaluation comparison against original episodes and concise summaries.

**Exit evidence:** minority exceptions survive, project facts stay scoped, executable and advisory procedures are both exercised, and supported adoption/deferral is automatic under configured evidence rules. **Tests:** T09, T06 integration and procedure transfer in T15.

### 22.13 WP12 — Maintenance and intentions

**Implement:** change interpretation, bitemporal revision, support reassessment, conflict investigation, indirect dependency triage, active-task reconciliation and all intention definition/occurrence/timer transitions. Connect trigger firing atomically to jobs and completion to checked evidence. Implement recurrence, expiry, cancellation and late results.

**Exit evidence:** all lifecycle/order fixtures pass; C/D historical results remain correctly scoped; repeated triggers have one occurrence; unknown effects do not become completion; source change/retirement drives appropriate current guidance. **Tests:** T10 plus T02, T08 and semantic maintenance tests.

### 22.14 WP13 — Retention, erasure and administration

**Implement:** live exposure registry, immediate revocation epochs, cross-store purge planner, support-versus-derivation handling, fenced Pi clean-session continuation, storage/artifact/cache purge and restore-time deletion registry. Complete profile/schema/session upgrade administration and restricted-content handling.

**Exit evidence:** deleted content cannot be retrieved, rerendered, reused or restored through covered paths; unrelated retained content survives; purge reports distinguish actual completion from pending provider/backup expiry; old tool effects do not replay during clean continuation. **Tests:** T12 and destructive-administration T03/T14.

### 22.15 WP14 — User and operator interfaces

**Implement:** all eight experiences in §18 through the shared API and CLI contract (web UI deferred by Theo); task evidence and context inspection, corrections, teaching, entity/history views, commitments, change briefs, temporary branches and prepared authority requests. Add policy/provider controls and notification delivery.

**Exit evidence:** unattended workflows continue appropriately; one scoped correction changes the intended future work; notification suppression preserves state; users see the basis/coverage actually recorded; silence remains unresolved. **Tests:** T13 and applicable authority/retention cases from T12.

**Delivered scope:** `UserRequest`/`UserMutation` use the existing authenticated command
endpoint and generated contracts. Corrections retain history and publish the WP12
workspace change notice. User contributions are attributed candidates; temporary
explorations are excluded from retrieval until an explicit promotion, which preserves
uncertainty. Named owners answer prepared requests; absent, declined or expired
answers block that job's execution admission. Notification preferences and receipts
only affect delivery. Policy revisions and provider/family/profile dispatch switches
are administrator operations. Assigned Pi workers publish their actual render
manifests for paginated task inspection. WP13 deletion covers these additional copies.

The [user guide](../user-guide.md) exercises these operations through a disposable
local Host and CLI. No new production dependencies or paid-service calls were needed.
This increment provides pull notification delivery and a worker library, not a web
UI, external notification adapter or worker supervisor. Scheduling/deployment remains
WP15; model quality qualification remains separate.

### 22.16 WP15 — Complete deployment and operational recovery

**Implement:** supervised local installation and production manifests; databases/object storage, trusted worker pools, Harbor service, secrets, tenant fairness, monitoring, backup/restore and local-to-production migration. Write and exercise every runbook in §20. Qualify the actual provider/image/runtime matrix.

**Exit evidence:** restart, worker replacement, database failover, complete restore and ownership transfer work with live operation state; deployment load reports state achieved bounds; teardown and retention jobs recover after outages. **Tests:** T14 with whole-stack fault injection.

**Delivered (18 September 2026).** *Worker pool:* administrators mint job-bound worker
credentials at runtime; a `pool` credential claims through `claim_next`, which orders
candidates by the number of active jobs per project (fair claiming) then age, mints the
worker credential first and claims as that actor; the Host spawns one Node pool child per
pool credential, restarts it with bounded backoff, stops it on shutdown, and runs coordinator
recovery every five seconds. The Node pool keeps interactive and deferred slots separate,
dispatches investigation (assigned Pi worker, strict `WorkResult` output, input-memory
delivery, stale input → `blocked`) and formation (one retry-safe window per source; the brief's
purpose names the operation), logs every judgement call, and releases a failed attempt's lease.
*Fault injection (T14):* a worker that dies after claiming → lease expiry → recovery → attempt 2
completes; a judgement provider outage → recorded unavailability and deferral → a new-purpose
job retains once the provider returns; a Host restart mid-run → pool restart → queued work
picked up (paid live run). *Backup/restore:* administrator `backup` writes a consistent SQLite
snapshot (`VACUUM INTO`), artifact copy, deletion registry and a manifest with recovery
position, schema version and checksums; CLI `restore` verifies the checksum, restores into
isolation with `restore_registry`, disables the pool, and the exercised test applies a deletion
recorded after the backup. *Operations:* `/healthz`, `/readyz`, `/metrics`. *Production
profile:* `deploy/` Dockerfile, compose with PostgreSQL role separation, file secrets and TLS
proxy — syntax-validated, not built or run here. *Migration:* `memory-migrate` copies a SQLite
snapshot into an empty PostgreSQL store in foreign-key order with the commit clock carried and
counts verified; tested against a disposable cluster. *Runbooks:* thirteen entries in
[docs/runbooks.md](../runbooks.md), each marked exercised or documented.

**Verification:** `cargo test -p memory-host` 16 passed (3 PostgreSQL-gated), `cargo test -p
memory-store` 9 passed (8 gated; the migration test passed under a disposable cluster);
29 targeted TypeScript tests (pool, faults, CLI walkthrough, backup/restore, examples, assigned
worker); fmt, Clippy and tsc clean; `docker-compose config` valid; one paid live run.

**Not delivered:** pool dispatch for activation, consolidation and maintenance — each needs a
process input (`ActivationQuery`, `CohortSelection` plus a synthesis model, `MaintenanceRequest`)
that `WorkBrief` cannot carry; the proposed contract addition (`process_input`) is deferred to the
agent story so the initiator that owns the input drives the design. Also not delivered: persistence
or rotation of issued credentials, a live production deployment or database failover drill, an
index-corruption drill, and qualification of the provider/image matrix beyond Docker and the
bounded Daytona profile already recorded.

### 22.17 WP16 — Whole-system qualification and release

**Implement:** assemble all conceptual scenarios and acceptance stories; run process substitutions, long histories, cache/result-reuse studies, all-family judgement qualification, user-effort tests and scoped-execution ablations. Freeze the supported version matrix and publish operational/semantic/behavioural results separately.

**Exit evidence:** §21's complete-release criteria; every traceability row resolves to delivered code, tests and evidence; the release specifies each Jev family's active mode and fallback; local and production profiles pass the same functional stories. No component's mock-only test is presented as a live integration result. **Tests:** T01–T15 and the release manifest.

**Delivered (18 September 2026):** the release package. `RELEASE.md` freezes the version
matrix, records every judgement family as `shadow` with the catalogue fallback and its live
evidence, separates operational, semantic and behavioural results, checks each §21.4
criterion, and keeps a not-qualified register. `traceability.md` gained WP10–WP15 location
sections so every requirement row resolves to code, tests and evidence or to a named gap. Both
release gates passed (`npm run check`: 38 Rust, 148 TypeScript, 28 bridge, 159 upstream;
`npm run test:store`: 22 Rust including migration, 57 PostgreSQL Pi backend).

**Open (in order):** the §21.1 agent story (process-input contract → Pi tool-event connector →
side-lane adaptive-work worker → orchestrated A/formation/consolidation/B run); unattended
consolidation and maintenance for §21.2; §21.3 as one continuous story; held-out qualification
per judgement family with thresholds; process-substitution, long-history, cache/reuse,
user-effort and scoped-execution studies; a production-profile run passing the same stories.
