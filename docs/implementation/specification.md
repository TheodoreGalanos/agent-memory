# Agent Memory
## End-to-end implementation specification

**Version 1.0 · Implementation design · 17 September 2026**

**Design basis:** the attached `specification.md`, including §§1–14 and Appendix A. This document implements that conceptual design together with the agreed Pi AgentHarness, Harbor and experimental ASP architecture. It defines the complete delivery scope, including local and production deployments.

> Pi manages agent execution. Harbor provisions execution environments. ASP connects their filesystem and shell boundary. The memory service owns retained meaning, policy and intentions. Explicit operations connect these responsibilities.

### Status and reading convention

This is a proposed implementation contract, not a report of a running integration. **[C §n]** identifies a requirement in the attached conceptual specification. **[U1]–[U13]** identify upstream sources checked for this design. Component names, wire contracts, schemas, algorithms and defaults introduced here are implementation decisions. Upstream API names appear in code formatting and are attributed where first introduced.

The conceptual document remains the authority for behaviour. This implementation adds storage layouts, service boundaries, recovery rules, concrete processing paths, tests and a dependency-ordered delivery plan. All work packages are part of completion; early packages establish dependencies rather than define a reduced product.

### Reading map

| Part | Implementation content |
| --- | --- |
| §§1–4 | Decisions, upstream reuse, ownership, data and service contracts. |
| §§5–9 | Durable coordination, Pi, Harbor/ASP, workspace rendering and scoped execution. |
| §§10–16 | Policy, all four memory processes, intentions and the complete Jev integration. |
| §§17–20 | Security and erasure, user interaction, evaluation, deployment and operations. |
| §§21–23 | Whole-system acceptance, dependency-ordered work packages and requirement coverage. |
| Appendices A–B | Concrete wire contracts, configuration records, source baseline and experimental dependency handling. |

## 1. Implementation decisions and system ownership

### 1.1 Complete target

Deliver one memory system with two deployment profiles. Both implement formation, activation, consolidation, maintenance, all four retained-memory families, bounded workspaces, entity-centred retrieval, bitemporal claims, intentions, autonomous user interactions, scoped RLM-style operations and the entire semantic-judgement catalogue. Production adds shared storage, worker replacement, tenant isolation and operational scale; it uses the same domain semantics and test fixtures. [C §§1–12; Appendix A]

### 1.2 Selected decisions

| ID | Decision |
| --- | --- |
| D01 | Rust owns the memory API, domain state transitions, policy evaluation, job scheduling, aggregate budgets and domain-effect receipts. |
| D02 | Trusted TypeScript workers embed Pi `AgentHarness`; Pi AI handles generative provider transport. Reuse Pi's operation state machine, Session model, lanes, hooks and supported tool implementations. |
| D03 | Pi owns its conversation and operation records. The memory service owns the memory collection. Neither maintains a competing copy of the other's authoritative state. |
| D04 | Harbor is the sandbox provisioner. A TypeScript `AspExecutionEnv` implements Pi's `ExecutionEnv` over the pinned ASP v0 SSH contract. This adapter is project code. |
| D05 | Local domain storage uses SQLite; Pi sessions use the upstream SQLite backend. Production uses PostgreSQL for domain storage and a project-owned PostgreSQL backend implementing Pi's storage/session contracts. |
| D06 | Use database-backed job delivery and an outbox. A dedicated workflow engine is an interchangeable later infrastructure option, not a prerequisite or an unimplemented dependency of this plan. |
| D07 | Large evidence, intermediate outputs and result artifacts use a governed artifact service: local files locally and object storage in production. |
| D08 | Start lexical and entity retrieval with ordinary indexes; implement an embedding retrieval adapter and scoped exact-search reference in the complete system. Production supports an indexed vector backend after parity and recall tests. |
| D09 | Jev is a replaceable judgement provider behind a typed adapter. All defined question families receive packet builders, policies, fixtures and a working baseline/fallback. Autonomous Jev use is enabled per evaluated family. |
| D10 | Use UUIDv7 identities and readable labels; use integer record revisions. Integrity checksums may protect transferred bytes. Identity and lineage use ordinary records and references. |
| D11 | One process owns a writable Pi Session at a time. Independently placed child workers use separate sessions; lanes share the session's ownership and access boundary. |
| D12 | Provide an authenticated API, a CLI and a small web interface for inspection, correction, teaching, commitments and operator control. Routine memory processing is autonomous. |

### 1.3 Runtime components

| Component | Owns | Inputs and outputs |
| --- | --- | --- |
| Memory service | Records, relations, intentions, policies, judgement decisions, read models. | Typed commands and queries; receipts, context groups and change events. |
| Host coordinator | Jobs, attempts, session leases, sandbox allocations, budgets, effect permission. | Work requests and events; worker assignments, permits, cancellations and results. |
| Pi worker | One or more exclusively owned sessions; agent operations; workspace projections. | Work briefs and referenced evidence; Pi outcomes and application result artifacts. |
| Harbor bridge | Provider lifecycle and provider credentials. | Sandbox specifications; allocated endpoints, capabilities and lifecycle observations. |
| ASP execution adapter | Filesystem/shell transport and operation-scoped process control. | Pi I/O requests; typed results, bounded output and artifact references. |
| Artifact service | Storage, access, provenance registration, retention and export. | Byte streams plus metadata; opaque references and bounded reads. |
| Judgement adapter | Packet execution, response validation, attempt/usage reporting. | Versioned packets; typed assessments and transport status. |
| Evaluation runner | Protected fixtures, reference world, graders and experiment manifests. | Pinned candidates; reproducible reports and qualification evidence. |
| User interface | Views and authorised commands. | Recorded state and results; scoped contributions and decisions. |

The memory service and coordinator are modules in one Rust application in the local profile. They can be deployed together or independently in production while retaining one authoritative transaction boundary for domain changes and their outbox. Trusted workers keep model credentials and narrow service clients; sandboxed code receives only assigned resources and capabilities.

### 1.4 Proposed repository structure

Use a repository-neutral structure, adapting names to the target repository's established conventions:

```text
contracts/                 shared wire schemas and generated clients
crates/memory-domain/      records, policy, processes, intentions, temporal rules
crates/memory-store/       SQLite/PostgreSQL repositories and migrations
crates/memory-host/        API, scheduler, leases, budgets, effects, artifact service
packages/pi-worker/       AgentHarness binding, rendering, tools, profiles
packages/pi-session-pg/   PostgreSQL storage/session adapter and conformance
packages/asp-env/          ExecutionEnv over SSH; bounded remote process protocol
packages/judgement/        packet builders, Jev/baseline adapters, response checks
services/harbor-bridge/    pinned Python Harbor lifecycle integration
apps/memory-ui/            inspection, corrections, commitments, operator views
cli/                      local lifecycle and inspection commands
policies/                 versioned policies, profiles and judgement definitions
evals/                    fixtures, graders, scenarios, experiments and reports
deploy/                   local/production manifests, backup and upgrade runbooks
```

These are ownership boundaries rather than mandatory independently published packages. Shared schema generation and contract tests prevent Rust/TypeScript/Python definitions from drifting.

## 2. Upstream reuse and compatibility contract

### 2.1 Verified baseline

The source baseline inspected is Pi commit `e4c75a73222ae2c72abb5f5314fa35ee8effc508` and Harbor ASP RFC pull request #3023, head `dd4784b1ade1f446399e194f6dffd15142a77b98`. The ASP pull request was open and unmerged when checked. Pi's current harness documentation describes storage format 4 as pre-stabilisation. [U1–U5]

The build manifest records the actual package/source revisions, lockfiles, runtime versions, database versions, image identities and dependency licences. Source inspection establishes available contracts; compilation and the integration suite establish which pinned package set the project supports. Resolve an installable package set or build pinned upstream sources during WP01. Record this as a tested compatibility manifest before subsequent packages depend on it.

### 2.2 Reuse map

| Upstream contract | Project use | Required adaptation |
| --- | --- | --- |
| `AgentHarness.create({session, models, ...}, context)` | Attach a harness to one owned Session and discover open operations. | Worker lifecycle, identity binding, profiles and policy hooks. |
| `AgentLane.accept`, `drive`, `getResult`, `requestAbort`, `inspectExecution` | Durable admission, advancement, observation and cancellation. | Coordinator maps host job identity to Pi operation identity. |
| Session, entries, values/lists, branch state, usage | Conversation and agent-operation persistence. | Application entries and backend adapters; preserve upstream transaction semantics. |
| `transform_context`, `toProviderMessages`, `entryProjectors` | Bounded context projection. | Origin-aware memory entries, rendering manifests and safety checks. |
| `before_compaction`, `before_request`, `before_tool`, `after_tool` | Compaction, budgets, effect checks and result bounding. | Idempotent hook integration; substantial work becomes explicit operations. |
| `AgentHarnessToolInvocation` | Stable tool identity, replay memos and progress checkpoints. | Bind domain receipts to session plus invocation identity. |
| `ExecutionEnv extends FileSystem, Shell` | Tool filesystem/shell abstraction. | `AspExecutionEnv`; preserve errors, paths, output bounds and cancellation. |
| Harbor `BaseEnvironment` | Sandbox lifecycle and provider abstraction. | Provisioning bridge that binds the resulting endpoint to a trusted descriptor. |

The current source exposes these harness APIs and extension points. Its specification separates scheduling and writable ownership from agent execution and documents uncertain external effects. [U1–U4, U6]

### 2.3 Required project-owned closure

The inspected upstream audit reports incomplete session-wide watch, search, telemetry and remote-session features; PostgreSQL session storage and precise rewrite require additional work. [U3] This implementation therefore makes the following choices:

**Live Session objects remain process-local.** Expose semantic worker commands, snapshots and events, not arbitrary remote storage mutation. Build user views through supported lane observation plus the host's domain API. Reconnect obtains a fresh snapshot with a cursor; a stale watch is discarded.

**Domain search is our own service.** It queries memory records and source references. Pi transcript search is not on the correctness path.

**Production session storage is a required adapter.** Implement Pi's storage contract over a separate PostgreSQL schema, with upstream conformance plus project ownership fencing. Keep domain transactions separate even when both schemas share one database server.

**Erasure is a required administrative workflow.** Use fenced session replacement and governed source removal as specified in §17. Do not equate context compaction with erasure.

**Required telemetry is ours to verify.** Adapt existing Pi events and usage records; instrument host, packet, render, sandbox and provider boundaries explicitly.

### 2.4 Upgrades and experimental dependencies

Keep ASP transport schema, Pi persisted format, worker build and domain schema independently versioned. A worker attaches only to a compatible session format. Pin in-flight sessions to their worker compatibility family until drained or migrated. Before an incompatible upgrade, test every persisted open-operation state represented in the project fixtures, along with custom entries and compaction results.

ASP configuration errors produce an unavailable sandbox binding. Tool calls never fall back to local host execution. Upstream changes are adopted through the conformance suite; project adapters absorb differences while the memory API remains stable. The complete system includes these compatibility mechanisms rather than assuming draft interfaces are stable.

## 3. Domain records, relationships and temporal storage

### 3.1 Common record envelope

Each retained record has a UUIDv7 `memory_id`, `family`, readable `label`, `tenant_id`, owning scope, creation source and current revision number. Each recorded representation has a `version_id`, integer `revision`, typed content, evidential status, source references, temporal metadata and an associated policy decision. A reference is `(memory_id, revision)`; human-facing text includes the label.

Keep separate dimensions for **origin** (observed, activated, agent-generated), **evidential status** (observation, attributed statement, inference, assumption, simulation), **availability** (routine, historical, retired, quarantined), and **procedure/check qualification** (candidate, evaluated for stated conditions, withdrawn). Changing one dimension does not implicitly change another. [C §§2–4, 9]

All protected queries begin with authenticated tenant and scope. User, project, task, entity and source revision are distinct fields or memberships, not labels packed into an unvalidated free-text namespace. Personal preferences can span the user's permitted work; project preferences retain the narrower context. Explicit user statements establish attributed preferences, while world claims retain their evidential assessment.

### 3.2 Logical schema

| Table group | Required content and constraints |
| --- | --- |
| `memory_records`, `memory_versions` | Stable record identity; family payload; revision; status; source/derivation references. Unique tenant/record/revision. Expected-revision checks on update. |
| `relation_records`, `relation_versions` | Typed endpoints and versions where required; direction; basis; scope; temporal interval; proposal or accepted status. |
| `entities`, `entity_aliases`, `entity_links` | Provider-native identity, scope, labels, aliases and supported cross-representation mappings. Candidate matching remains separate from accepted identity. |
| `sources`, `source_versions`, `source_locators` | External source owner, revision, acquisition method/time, snapshot reference and stable content locator. |
| `artifact_records`, `artifact_dependencies` | Storage key, media type, size, origin, version, retention class, protection scope and dependent outputs. |
| `policy_versions`, `policy_decisions` | Configuration, permitted uses, effective period; inputs to a decision, selected disposition and reason. |
| `jobs`, `job_attempts`, `session_bindings`, `session_leases` | Host work state, retries, Pi mapping and exclusive ownership epoch. |
| `effect_requests`, `effect_receipts` | Idempotency scope, canonical request, outcome knowledge and external reconciliation references. |
| `intentions`, `intention_occurrences`, `intention_transitions` | Purpose, readiness, trigger, recurrence, expiry, checked completion and state history. |
| `outbox_events`, `inbox_receipts`, `timers` | Transactional publication, per-consumer deduplication, delivery cursors and due work. |
| `judgement_definitions`, `judgement_packets`, `assessments` | Question versions, packet manifests, raw typed responses, provider identity and transport status. |
| `render_manifests`, `memory_uses`, `result_artifacts` | What entered each context, actual use declarations, bounded returns and lineage. |
| `budget_accounts`, `budget_reservations`, `usage_facts` | Aggregate allowance, outstanding permits, known charges and unresolved usage. |
| `deletion_requests`, `deletion_targets`, `revocation_epochs` | Immediate access changes, purge progress and independently checked completion. |

Use relational columns for identity, joins, access filters, time and state. Use validated JSON payloads for family-specific content and bounded model observations. Both backends implement the same domain repository contracts. SQL indexes cover tenant/scope, entity/source revision, valid/transaction time, event cursor, due timers, job readiness and relation endpoints.

### 3.3 Family payloads

**Episode:** objective; bounded event range; initial conditions; observations and actions; corrections; outcome and its verification; source locators; remaining uncertainty. An episode can reference multiple artifacts and modalities. Synthetic or reconstructed experience is marked at creation and retains that status through derivation.

**Knowledge:** a claim or concept, relevant subject/predicate where available, content, source support, uncertainty, entity scope and time. A statement of absence records the examined coverage and basis. Unresolved or conflicting claims remain individually addressable.

**Procedure:** executable or advisory form; purpose; required capabilities; applicability and exclusions; inputs; steps or artifact entrypoint; assessment criteria; evidence; counterexamples; qualification history. Executable artifacts are versioned and execute in the assigned sandbox. Advisory methods specify observable investigation and evidence criteria.

**Intention:** purpose; authorised owner; trigger specification; readiness predicates; completion specification; expiry; notification policy; linked records and recurrence. The occurrence lifecycle is implemented in §15.

### 3.4 Relationship rules

Implement `supports`, `challenges`, `conflicts with`, `derived from`, `depends on`, `supersedes` and `triggers` with the meanings in C §4. Symmetric conflicts use a canonical endpoint ordering to prevent duplicate edges. Directional edges preserve direction. A relation has its own evidential basis; an inferred relation is distinguishable from an explicit identifier mapping.

`derived from` identifies actual production inputs; `supports` expresses their evidential role. Preserve common-source ancestry during support assessment. A source reused by several workers remains one source. Derivation cycles introduced by a new record are rejected; other graph cycles can be legitimate and traversal uses visited sets and bounds. Scope changes and cross-project transfer create an explicit authorised contribution, with retained restrictions on its evidence.

### 3.5 Bitemporal implementation

Store valid intervals as half-open `[valid_from, valid_to)` and transaction history using database-assigned commit sequence plus UTC timestamp. Open ends use null; unknown valid time is explicit rather than guessed from ingestion time. Source revision identity is a separate applicability condition.

A query accepts `valid_at`, `recorded_as_of` and scope. It first selects the representations known at the transaction cutoff, then selects the applicable valid-time intervals. Current queries use the latest visible transaction position. A database sequence gives deterministic ordering when timestamps tie.

On correction, acquire the record's expected revision, close the previous transaction interval and insert the new representation in one transaction. A late-arriving world change can narrow an earlier claim's valid interval and insert its successor as a linked claim. Separate assertion records can represent split valid intervals; compatible facts and unresolved competing claims are not forced into one unique current value.

Example: C's claim is learned on the 8th; a D change effective on the 10th is learned on the 12th. A recorded-as-of-the-11th query still returns the old recorded position. A current query about the 10th reflects the newly learned valid-time boundary. A completed report about C retains its own source revision and verification basis. [C §4; §9]

### 3.6 Source and artifact durability

A source locator records the representation used: document passage/page and optional region, table/row/field, model entity/property, image region, event range or tool-result fragment. Preserve the source revision, acquisition method and snapshot when retention allows. Reopening a source either supplies that version or reports its unavailability.

Artifact writes follow `allocate → stream to temporary object → verify completion → publish reference`. Database records that mark an artifact ready are committed only after its bytes are durable. Failed publication leaves an orphan eligible for cleanup; a ready reference never intentionally points to an incomplete object. Transfer checksums verify bytes, not semantic truth.

Artifacts are addressed through the service, not arbitrary filesystem paths or object-store keys. Access is checked on each read, export and materialisation. Full logs and model packets follow the same retention rules as the source content they contain.

## 4. Service contracts and command boundaries

### 4.1 Common command envelope

Define shared wire schemas in `contracts/` and generate typed clients. Rust validates every external payload and authenticates worker identity. The minimum envelope is:

```text
request_id; command_version; tenant_id; actor_id; scope;
job_id?; session_id?; lane_id?; operation_id?; invocation_id?;
expected_revisions[]; deadline; budget_id; lease_epoch?;
payload
```

Tenant, actor and effective authority are established from authentication and host assignment, then checked against the envelope. The worker or model cannot broaden them by choosing different values. Idempotent mutations include a stable caller-provided command ID and a request equivalence check; reusing an ID with different arguments returns `conflict`.

Responses distinguish `accepted`, `completed`, `partial`, `blocked`, `conflict`, `unavailable` and `failed`, with a typed reason, result reference, known effects, evidence coverage and usage status. Accepted work returns an ID whose status can be queried after a disconnect. Partial results identify unmet conditions.

### 4.2 Public and worker-facing operations

| Group | Proposed service operations |
| --- | --- |
| Tasks/workspaces | Create task, submit input, inspect status/context, resume, request cancellation, create temporary branch, select contributions. |
| Sources/artifacts | Register source revision, publish observation, allocate/upload/finalise artifact, read a bounded region, export permitted evidence. |
| Memory | Read versions, query entity/history, retrieve candidates, submit contribution, apply correction, inspect support, transfer selected method. |
| Processes | Request formation/activation/consolidation/maintenance, inspect result, request additional evidence or follow-up. |
| Intentions | Create, amend, arm through maintenance, deliver trigger, inspect, renew, cancel, submit completion evidence. |
| Judgement | Prepare/execute packet, inspect assessment, evaluate policy response, register task-local check, request qualification. |
| Operations | Claim/renew/release assignment, report Pi operation mapping, reserve usage, submit receipt, publish bounded result. |
| Administration | Policy/profile activation, access change, retention/deletion, migration, backup, restore and health. |

Use an HTTP JSON API with an OpenAPI description for these project contracts; local clients connect over loopback or a Unix-domain transport, and production uses authenticated TLS. Streaming task output uses a cursor-based event channel with reconnect snapshots. A CLI and web client use the same commands as agents under their own permissions.

### 4.3 Model-facing tools

Expose narrow tools for `memory.search`, `memory.inspect`, `memory.contribute`, `workspace.inspect`, `artifact.read`, `operation.spawn`, `operation.result`, `judgement.assess`, `intention.propose` and `task.finish`, alongside selected Pi execution tools. Names are proposed project tools, not claimed Pi exports.

Tool results contain bounded findings and references. Mutating tools pass through policy and the receipt boundary. Agent-supplied SQL, arbitrary service URLs, raw object-store credentials and unscoped record browsing are excluded from the capability set. Structured query operations expose only allowed filters, projections and joins.

### 4.4 Compatibility and cancellation

Version payload schemas independently from stored representations. Additive wire changes remain backward compatible; incompatible changes create a new command version. Deadline cancellation aborts the request, while durable task cancellation is a separate explicit command. A lost client connection does not erase accepted work or imply cancellation of its external effects.

## 5. Durable coordination, budgets and effects

### 5.1 Separate the state machines

A **host job** coordinates work across sessions, services and sandboxes. A **Pi operation** advances one agent lane. An **intention occurrence** records a commitment and checked outcome. Store explicit references between them; successful job execution does not itself complete an intention, and a Pi terminal result does not by itself prove a domain change committed.

Host job states are `queued → leased → running → waiting | completed | partial | failed | cancelled`. A waiting job has a reason, wake condition and bounded deadline; waking returns it to `queued`. Attempts retain individual outcomes. Interrupted work whose external outcome is unknown enters a reconciliation wait rather than being classified as a clean failure. Cancellation has its own requested flag until host effects are reconciled.

A job stores its kind, scope, parent/root, brief, inputs, output contract, policy/profile versions, remaining allowance, readiness, deduplication key and Pi mapping. Job kinds include all four memory processes, scoped investigation, evaluation, sandbox lifecycle, deletion, notification and migration. Pure structured jobs can finish without creating a Pi session.

### 5.2 Durable delivery and the Pi admission handshake

1. Commit the accepted job and its outbox event together. An API acknowledgement means accepted work is durable.
2. Claim an eligible job with a bounded attempt lease. Production claims use row locking and `SKIP LOCKED`; no database transaction stays open while a model or sandbox runs. Local claims use an equivalent short SQLite transaction. [U10]
3. Assign an exclusively owned Session and stable proposed Pi operation ID. Record the mapping before sending `accept`.
4. The worker checks `getResult`/`inspectExecution`, then calls `accept` with the mapped ID where admission is still required. A lost acknowledgement is resolved by inspecting the same operation and persisted request; it does not create a fresh operation.
5. Call `drive`. A durable retry/deferred wait becomes host readiness metadata; the coordinator schedules a later drive. A settled Pi result becomes a candidate job outcome.
6. Publish the application result artifact and commit the host result/receipt. Repeated result delivery returns the existing receipt when content and scope agree.

The worker adapter serialises admission per lane and handles `LaneBusy` and mismatched operations explicitly. The source exposes optional caller-supplied operation IDs and distinct admission/drive interfaces; their retry semantics are verified in WP04. [U1]

### 5.3 Ownership and fencing

Local ownership is enforced by a single supervised host and an exclusive session lock held for the worker's lifetime. The supervisor closes/kills the previous owner before reassignment. The lock protects the actual canonical session storage identity, not just a display ID.

Production ownership uses `session_id`, monotonic `lease_epoch`, `owner_id`, expiry and heartbeat. A new owner increments the epoch atomically. Every session-storage mutation and domain-effect permit verifies that epoch at its commit boundary. The PostgreSQL Pi backend obtains a lock on the lease row while validating and committing its writes. A process-local Session that loses the lease is closed and discarded. A pre-call heartbeat alone is insufficient fencing.

The same epoch binds sandbox execution. On owner loss, revoke the old connection capability, stop or isolate its remote process group, and establish the old sandbox's state before making a replacement writable. A new worker is not given shared write access to an unfenced old interpreter. Expired leases are detected with server time; monotonic clocks govern local elapsed-time limits.

### 5.4 Domain mutations and outbox

A domain commit atomically validates authorisation, revocation epoch and expected record versions; writes the state change; creates the operation receipt; and appends scoped change/outbox events. Event delivery is at least once. Consumers record `(consumer_id, event_id)` with their resulting work to deduplicate effects. Event payloads contain IDs, versions and change classification; protected source text is fetched through authorised APIs.

Use event cursors for subscription catch-up. An offline task can reconcile from its references and the current record state even after older events have expired. Jobs, event history and session transcripts each have their own retention policy.

### 5.5 External effect protocol

Each effect request has a stable key `(tenant_id, logical_operation_id, effect_kind)` plus the canonical arguments, scope, authorisation and expected versions. Pi tool invocations bind this key to `(session_id, invocationId)`; attempt number does not change the logical effect identity.

Classify an effect as **replay-safe observation**, **idempotent mutation with a remote key**, or **mutation requiring reconciliation**. Record `prepared → in_progress → succeeded | failed | outcome_unknown`, with observed receipts. For idempotent remote services, submit the stable key and reuse its receipt. For an unknown outcome, query the owning service or inspect the sandbox result before repeating the action. A non-idempotent action with no reconciliation path remains unresolved and follows policy.

Pi's tool replay setting is based on this implemented behaviour, not the tool's name. Generic shell commands default to replay-never because they can mutate state. A verified read-only wrapper may declare safe replay. Model calls can be billable even when their response is lost; record usage as unknown and preserve the attempt rather than inventing a zero charge. [U2]

### 5.6 Aggregate resource enforcement

A root budget contains limits for currency, tokens, wall time, model calls, sandbox CPU/time, concurrent children, recursion depth and output size. Every child draws from the same root; a child allocation is a reservation, not a second independent allowance. Reserve capacity for a bounded final result before starting discretionary work.

Before a provider call, atomically reserve its maximum configured allowance, including permitted retries. Afterward, replace the reservation with observed usage or an explicit unresolved amount. Repeated usage reports deduplicate by provider-attempt identity. Costs roll up once by parent relationships; reports can show child breakdowns without adding them twice to the root. Provider price tables and estimators are versioned configuration.

Use provider-side output caps, bounded retry counts and sandbox limits where supported. Budget estimates are reconciled with observed billing; enforcement cannot promise a perfect invoice cap for providers without hard billing controls. Such uncertainty remains visible in admission policy. Generative transport, Jev, packet construction, artifact processing and evaluator calls all pass through the same accounting path.

### 5.7 Recovery scenarios

| Failure point | Required recovery |
| --- | --- |
| Accepted host job; Pi admission acknowledgement lost | Inspect the mapped operation and result; admit only if absent and the lane permits it. |
| Domain change committed; tool response lost | Read the receipt by stable effect identity and return the recorded result. |
| Provider response settled in Pi; worker dies before host report | Reattach compatible harness, read settlement/result and report without requesting another model response. |
| Remote command may still be running | Inspect its remote execution ID, stop or reconcile the process and artifacts; classify uncertain effects explicitly. |
| Artifact ready but database publication fails | Retry publication with the same identity or clean the unreferenced artifact after its grace period. |
| Host job complete but notification delivery fails | Retry the outbox delivery independently; the completed work remains complete. |
| Session lease changes during a write | Storage commit rejects the stale epoch; replacement ownership proceeds through fencing and recovery. |

## 6. Pi integration and session persistence

### 6.1 Worker lifecycle

A worker receives an authenticated assignment containing the session ID, ownership epoch, job/operation mapping, model/profile versions, sandbox binding and scoped service capabilities. It opens its Session through the selected repository, attaches `AgentHarness`, enumerates open operations, and reconciles them with host jobs before accepting new work. Only the host schedules active drivers.

Use one main lane per task session by default. Related exploratory lanes can share that session when their evidence access and lifetime agree. Independent child jobs or stricter disclosure boundaries get new sessions with an empty root and a supplied brief. Inheriting a parent tip is an explicit operation, not the default for restricted children. [C §§3, 5; U1–U3]

### 6.2 Reused storage semantics

Pi's entry tree, bound values/lists and usage ledger remain Pi-owned. The worker uses upstream Session mutation and storage interfaces; it does not reconstruct the operation state from ad hoc application events. Application custom entries contain references to work briefs, rendered memory groups, result artifacts, corrections and unresolved obligations. Keep large payloads in governed artifacts and render bounded contents when needed.

For local delivery, use the pinned upstream SQLite Session backend, with tested database settings and per-session retention administration. Domain state resides in a separate database. An application directory has an installation manifest, `memory.sqlite`, session storage, artifact storage and protected connection configuration.

For production, implement `PiPostgresStorage` and repository bindings in TypeScript against the pinned Pi storage/Session contract. Use a separate `pi_sessions` schema, transactionally ordered entries, bound values/lists, branch state and usage. Enforce the host lease epoch on every mutation. Use a per-session monotonically increasing write sequence under row locking. Preserve list cursors, all-or-none commit and visibility/publication ordering through the upstream Session layer. Add adapter-specific indexes and bounded queries without changing upstream logical payloads.

The backend passes upstream conformance, local/PostgreSQL fixture parity, reconnect, cancellation, transaction-failure and stale-owner tests. It is a required production deliverable, not an assumption about an existing Pi export. Avoid requiring a raw remote `Session` API: the live object and harness remain local to the trusted worker.

### 6.3 Hooks and context boundary

| Hook or interface | Binding in this implementation |
| --- | --- |
| `before_drive` | Check current host assignment and operation eligibility before advancing. Storage/effect boundaries independently enforce fencing. |
| `before_run` | Add the work brief and initial bounded context references. |
| `entryProjectors`, `transform_context` | Interpret application entries and produce the logical selection for rendering. |
| `toProviderMessages` | Produce a valid provider transcript and associate the render manifest. Preserve tool-call/result structure. |
| `before_request` | Acquire the call allowance and final disclosure permit; apply bounded provider options. |
| Provider transport wrapper | Capture actual request identity/usage, distinguish retries, enforce the final allowlist and size limits. |
| `before_tool`, tool wrapper | Validate arguments and invoke the host's permission/idempotency path. |
| `after_tool` | Bound display content, preserve artifact references and application evidence. |
| `before_compaction` | Supply or validate a compaction preserving goals, origins, versions, conflicts and obligations. |

These hook names and options are present in the inspected interface. Final provider payload inspection is a compatibility test because provider transforms may alter size and prompt layout. [U1]

Hooks that can rerun before their result is settled use operation-scoped idempotent requests. Substantial consolidation or investigation is an explicit child job; it does not hide an unbounded workflow inside a context-transform callback.

### 6.4 Tools, memos and result boundaries

Reuse Pi's supported model-facing execution tools with `ExecutionEnv` in their tool context. Each application tool receives the harness invocation identity and uses its durable memo for bounded replay information such as the host command ID or result-artifact reference. A progress checkpoint describes progress; only a checked receipt describes a completed effect. [U4]

Model-facing tools provide a narrow evidence query or task action. `task.finish` returns the application result contract with scope, findings, cited inputs, unresolved work and coverage. A Pi run ending normally is treated as a completed agent operation, then the responsible process checks whether the application result meets its requirements. A missing or malformed result yields a bounded repair or a partial application result.

### 6.5 Compaction, custom entries and resumed work

Register explicit projectors for each application entry type. Test them through ordinary rendering, compaction, branch summarisation and restoration. If default compaction omits required custom content, use the supported hook to supply a project compaction result, preserving Pi's lifecycle and settlement.

Resuming a task reconstructs working state from Pi plus referenced artifacts, checks current access, and evaluates relevant substantive changes under the task's scope/freshness rules. Host job records do not mirror every transcript item. Context modifications are explicit appended updates or a declared rebuild; source history remains distinguishable from current task interpretation.

### 6.6 Storage upgrades and session administration

Keep session format and worker compatibility in the session directory/catalog. Upgrades either finish compatible open operations on their pinned worker or run an explicit offline migration under exclusive ownership. Preserve a verified restore point until the migration succeeds and retention permits its existence. Destructive administration first revokes execution, closes the owner and reconciles outstanding effects. Session erasure and clean continuation are specified in §17.

## 7. Harbor provisioning and experimental ASP execution

### 7.1 Provisioning contract

The host submits `SandboxSpec`: owner scope, job/session binding, resource ceiling, network policy, image reference, permitted artifact mounts/copies, working directory, capabilities, retention horizon and requested execution mode. The Python Harbor bridge translates this to the pinned provider environment interface, records the resulting provider ID and returns observed capabilities and connection status. [U5–U6]

Select Docker locally and one SSH-capable managed provider for the production profile, with Daytona as the initial qualification target. Provider selection is configuration backed by measured capabilities, not a claim that every Harbor provider supports ASP. Deployment qualification includes network controls, cancellation, filesystem behaviour, artifact export and lifetime. A failed capability check blocks the affected job with an explicit alternative or partial outcome.

Harbor trial/configuration types are contained inside the bridge. The memory host never needs evaluation-only objects to perform a domain operation. Providers, images and the bridge run with explicit resource and secret boundaries. Teardown is retried as a durable host job; the provider allocation is labelled so orphan cleanup can discover it.

### 7.2 ASP descriptor and trusted binding

Pin ASP v0's schema: `version`, `transport`, SSH connection host/port/user, optional identity reference, host key and absolute workspace. The reference RFC leaves provisioning to the orchestrator and uses a fresh shell per command with a persistent filesystem. [U5]

The host produces the descriptor in a trusted configuration area after allocation. Bind it to the sandbox ID and current session ownership epoch. Validate the endpoint, transport, expected host key and workspace against the allocation receipt. Use pinned host keys and narrowly scoped SSH identities; credentials remain outside model-visible artifacts. Prevent workspace-controlled descriptor discovery from overriding this binding.

Unsupported descriptor versions, unreachable endpoints or failed host verification produce `sandbox_unavailable`. The worker keeps execution tools unavailable until repaired. Disabling SSH forwarding, unmanaged proxy commands and incidental local SSH configuration is part of the adapter profile.

### 7.3 `AspExecutionEnv`

Use a supervised OpenSSH client and SFTP for the pinned ASP experiment, with a bounded remote helper where process/output control requires it. Keep agent command text on the remote input path rather than interpolating it into a host shell. Implement every filesystem/shell operation needed by the selected Pi tool set through this adapter. Pi's interface includes text/binary reads, line readers, writes/appends, rename, metadata, directory listing, path resolution, existence, directory creation, removal and temporary files. It returns typed errors rather than thrown filesystem failures. [U4]

Preserve remote POSIX path semantics. Relative paths resolve within the assigned sandbox workspace. The sandbox may contain its runtime filesystem; it never exposes host paths. Symlink checks concern mounted-data permissions and the remote namespace, and follow the selected tool contract. Transfer binary data as bytes, not interpolated shell text. Atomic rename is promised only within the supported remote filesystem. Map remote errors to the declared Pi categories.

Shell output is bounded at the producer/transport side as well as at rendering. Preserve the selected head/tail and line/byte limit, exit status, signal, truncation and full-output reference. A full output artifact is exported before teardown when its retention purpose requires it. No unbounded buffer sits between the remote process and the bounded tool result.

### 7.4 Execution identity, cancellation and uncertain outcomes

ASP supplies connection and raw I/O semantics. Add a project `RemoteExecution` wrapper above that transport: execution ID, sandbox generation, process-group ID, start time, deadline, output reference and terminal marker. The wrapper records a remote start/finish marker atomically where practical. It is a project extension, not a new claim about ASP v0.

On disconnect, inspect the same execution ID. Cancellation sends termination to the remote process group, escalates under policy and obtains a terminal observation. Killing the local SSH client alone is not reported as remote cancellation. If status cannot be established, fence the environment and mark the outcome unknown. A replacement sandbox is created only after effect policy permits it.

### 7.5 Artifact-oriented and interpreter execution

Support artifact-oriented commands for all operations: read referenced inputs and write explicit outputs. Also implement a persistent interpreter service inside a scoped sandbox for RLM workloads that benefit from variables across calls. The interpreter has an explicit session ID, bounded request protocol, idle/absolute lifetime and resource limits. It exposes structured inspection and result references; generated code cannot access host provider credentials.

Persisted files and registered result artifacts are recovery material. Arbitrary interpreter heap state is ephemeral. Checkpoints explicitly serialise approved application objects; recovery starts a new interpreter and restores those objects or reruns only replay-safe, recorded computations. Pending non-idempotent code receives reconciliation, not blind replay. The task is told when reconstruction has changed the available working state.

### 7.6 Sandbox-to-service capabilities

Generated code can call permitted structured operations and model subcalls only through a scoped capability proxy or a worker-mediated request channel. The proxy authenticates job, sandbox generation, scope and allowance, validates bounded requests and logs source/result references. Network deny-by-default is implemented in the provider environment with only the required service path enabled. Broad database, model-provider, cloud-admin and Docker-socket access stays outside the sandbox.

These permissions also govern package installation. Worker profiles use pinned images/resources; a model may request an installation as a scoped operation whose policy determines whether it is allowed. Installation results and image differences enter the operation evidence.

## 8. Workspace state, rendering and caching

### 8.1 Logical workspace

Represent goals, governing constraints, observations, hypotheses, plan, active obligations, open dependencies, available artifacts and selected memory as typed workspace entries. Pi persists the active execution view; large material remains referenced. Each entry records origin, evidential status, scope and lineage. A returned child inference retains its generating operation and supporting observations. [C §3]

Workspace recovery purpose and retention are independent of memory-family promotion. Temporary exploration can read permitted persistent knowledge while disabling formation from its assumptions and outputs. Explicit contribution selection re-enables retention for the chosen material only. Fresh child contexts receive the minimum complete frame, not a hidden copy of parent history.

### 8.2 Rendering algorithm

1. Reconcile current access, policy and relevant task-change events.
2. Compute available capacity from the provider/model profile, including output reservation, tool descriptions and anticipated results.
3. Keep governing constraints, the active goal, due obligations and decision-relevant uncertainty. Attach current source/record versions.
4. Treat unresolved conflicting claims and their status as a context group. Keep a concise representation of both positions and a route to inspect supporting detail.
5. Select remaining evidence and methods by the activation result and policy, using stable order for unchanged items.
6. Produce the provider transcript, validate role/tool-call pairing and estimate/tokenise through the selected provider adapter. Oversize context becomes a bounded next investigation with explicit deferrals.
7. Persist a `RenderManifest` before the request: selected contents/references and versions, origin, source coverage, tokenizer/model profile, estimated budget and rendering strategy. Capture final request accounting without placing credentials in the manifest.

Required observations and context groups may be compacted at semantic boundaries; exact IDs, scope and unresolved obligations are carried structurally. If the provider lacks a reliable tokenizer, use a conservative estimator and declared margin, then test actual request acceptance. No claim of exact token accounting is made for unsupported providers.

### 8.3 Cache-aware layout

Keep stable instructions, tool definitions and established task context in consistent form when the provider can reuse them. Append new evidence and explicit corrections during a work phase. Rebuild at task transitions, substantial budget pressure, access changes or accumulated obsolete material. Record the reason and cost. Providers can render system/tool changes differently, so cache behaviour is measured at the actual payload boundary.

Cache reads, writes where applicable, uncached input and output are separate usage facts. Prompt-prefix reuse, judgement-result reuse and investigation-result reuse have separate keys and metrics. Missing cache metrics are unknown, not a fabricated hit rate. Caching never delays a required access revocation or material correction.

### 8.4 Inspectable context and active dependencies

The task view shows selected memory and declared application using recorded manifests and citations. It distinguishes what the parent read from delegated findings. Track dependencies of unfinished conclusions locally to the workspace; expire them with the workspace's purpose. Reconciliation compares versions and meaningful change classifications for those references. A wording-only change or a different valid-time scope preserves an unaffected working result.

Completed reports keep their source and verification basis. Review after completion occurs through a request, explicit retained obligation or policy. Bitemporal collection history and retained session context support different historical questions; expose that difference in the API.

## 9. Scoped RLM-style operation runtime

### 9.1 Work brief and result contract

A `WorkBrief` contains purpose, owning process, task frame, definitions, entities/revisions/time, evidence references, known conflicts, permitted expansion, expected result, child permissions, policy/profile versions and root allowance. The worker validates its completeness before reasoning. Missing definitions can be fetched within scope; missing user intent becomes an explicit request only when it materially controls the task.

A `WorkResult` contains status, bounded findings, coverage, source/record versions, supporting and challenging evidence, proposed changes, applicability, unresolved questions, child-result references and observed cost. The full artifact remains inspectable; a bounded return is delivered to the caller. Partial work states what was examined and what remains, including any effects already performed.

### 9.2 Execution strategy

A worker can perform structured inspection, call a focused generative model, request a typed judgement or create a child operation. Exact entity/source/time filtering precedes broad semantic comparisons. Child work receives its own context and an allocation within the root allowance. Parent scope and permissions constrain every descendant.

Use explicit decomposition objects linking question, selected evidence and required outputs. Aggregation checks original coverage, supporting conditions, counterexamples and unresolved disagreements. An answer assembled from children retains each child's evidence cutoff and applicability. Missing child output never counts as a negative finding or completed coverage.

### 9.3 Waiting, fan-out and cancellation

`operation.spawn` creates idempotent child jobs and returns IDs. The parent can yield rather than occupying a worker while children wait. A job resumes when its dependency predicate holds or its deadline requires a partial result. Fan-out, depth and concurrent provider calls are bounded per root and tenant. Cancellation propagates to children according to ownership; shared reusable jobs retain an explicit subscriber/lifecycle rule.

Use separate sessions for independently scheduled child workers. Parent and child communicate by governed artifacts, not shared mutable message arrays. Within one trusted session, lanes can support related branches, but the session still has one writable owner. Persistent interpreter work obeys §7's lifetime and recovery rules.

### 9.4 Reuse and semantic completion

Reuse checks actual input versions/content, question/method definition, model identity where relevant, scope, permissions and required freshness. Structural transformations can reuse exact results; semantic results require compatible interpretation conditions. The record states whether result coverage meets the new question. Promotion from temporary artifact to retained memory follows formation or consolidation.

A completed worker operation is a candidate result for the owning process. Process-specific validation controls the subsequent domain commit. The same runtime supports in-task, alongside-task and post-processing execution; timing is a policy choice independent of context placement.

## 10. Policy and common processing machinery

### 10.1 Policy structure

A `MemoryPolicy` is a versioned configuration with retention purpose, allowed scopes, source/authority rules, mandatory evidence requirements, applicability rules, budget classes, scheduling priorities, judgement dispositions, notification preferences and qualification requirements. Separate operator-enforced constraints from task-specific preferences. Model proposals can supply interpretations and priorities; policy changes require the actor's relevant authority and a recorded version.

Evaluate policy through a deterministic Rust decision function over typed facts and recorded semantic assessments. Its output is `PolicyDecision { action, reason, constraints, required_evidence, policy_version, expires_at? }`. Candidate actions include retain, qualify, retrieve further, investigate, revise, defer, retire and request designated input. The initial ranking/routing policy uses explicit configured rules as required by the conceptual source; a learned value policy is not silently substituted. [C §6]

### 10.2 Common operation pipeline

Every process implements `prepare → inspect → assess → propose → validate → commit → publish`. Structured work can pass directly from inspection to a validated proposal. A `ChangeProposal` carries its process, evidence cutoff, source/record versions, intended scope, changed fields/relationships, evidential status, preserved distinctions, requested effects and output coverage.

Validation checks schema, authority, source existence, actual evidence included, scope/time compatibility, required assessments, qualification policy and expected revisions. Validate the whole change batch before applying it. A stale proposal is reassessed only against the relevant changed inputs; unaffected evidence remains usable. Commit state, policy decision, receipt and outgoing events atomically. Unresolved proposals remain candidate artifacts under their own lifecycle.

Each process defines its own semantic criteria. A structurally valid proposal is not automatically a verified world claim. A model score supports a tested policy route, while observed evidence and verification records retain their actual role.

### 10.3 Source ingestion and event boundaries

Connectors publish revisioned observations through a `SourceAdapter` contract: resolve identity, enumerate versions when available, read a bounded locator, fetch bytes, interpret source precedence supplied by the source owner, and return operational receipts. Implement a filesystem/document adapter, a structured model/tabular adapter and a tool-event adapter in the complete delivery. Domain-specific proprietary connectors can plug into the same contract.

Imported trajectories use an explicit format/version adapter, including ATIF where supplied. Capture observed calls/results and declared statements distinctly. Preserve evidence cutoffs for replay and evaluation. Missing telemetry or unavailable source bytes become explicit unknowns; source history is never inferred from the agent's prose alone.

Bound formation windows by meaningful events and cross-window references. Use event IDs and cursors for incremental processing so scheduled work advances from the last processed window instead of rereading the whole project.

## 11. Formation implementation

**Owner:** formation module. **Primary artifacts:** episode candidates, scoped claims, candidate procedures and intention proposals. **Triggers:** completed or corrected subtasks, consequential observations, explicit contributions and scheduled bounded ingestion. [C §5, §12]

### 11.1 Processing steps

1. Resolve the evidence window and permitted artifacts at a defined cutoff. Include the objective, governing conditions and surrounding events necessary to interpret the selected event.
2. Apply the retention policy to determine required, useful optional and temporary information. An empty contribution is a valid result when the experience has no retained purpose.
3. Use explicit event structure first; use a bounded judgement or worker to identify ambiguous event boundaries, contribution kinds, commitments and local interpretations.
4. Construct typed candidates. Keep observation and interpretation distinct, preserve correction sequence, and scope claims to inspected entities/revisions. A single episode can support several linked contributions.
5. Check claim support and scope against actual evidence, using established judgement policies or a generative fallback where needed. Explicit origins and actor identity come from trusted metadata.
6. Validate and commit the selected batch with source/derivation links. Advance the formation cursor in the same domain transaction as the batch receipt, or record per-window completion for independently processed windows.

### 11.2 Duplicate handling and enrichment

Idempotency is based on the observation/window plus policy operation identity. Reprocessing the same window returns its existing result or creates an explicitly requested re-interpretation revision. Similar text is a candidate-link signal, not a reason to collapse distinct episodes. Additional independent observations can enrich support through new records and links; repeated summaries keep their common origin.

Direct user preferences and authored methods are valid inputs. Persist the stated scope and distinction between explicit contribution and inferred preference. A task-local exploration's retention flag suppresses automatic promotion while allowing explicit selection of particular methods or findings.

### 11.3 Completion and tests

A formation result reports retained records, deferred candidates, essential coverage, unresolved interpretation and cost. Unit tests validate schema, references, retained cursor, scope, deduplication and atomic batch behaviour. Semantic tests compare supported content and required-information coverage at pre-correction and post-correction checkpoints. Held-out continuation tests determine whether the encoding helps later work. Synthetic source identity and simulated experience status survive every derived record.

## 12. Activation and retrieval implementation

**Owner:** activation module. **Primary artifact:** a `ContextPackage` with eligible candidate provenance, ranked context groups, applicability and due work. **Triggers:** task request, bounded context need, source/memory event or intention trigger. [C §§3–6]

### 12.1 Retrieval plan

Translate the current goal and explicit question into a typed retrieval plan: tenant/scope, entities, source revisions, valid/recorded time, record families, task freshness and candidate budget. Every search stage applies access constraints before scoring or exposing results.

Run eligible exact/entity lookup, lexical search and configured vector retrieval. SQLite FTS5 and PostgreSQL text search are backend adapters; an embedding adapter records model/version/dimension and source representation. The local reference path performs exact scoring over permitted candidates. Production may use an approximate index such as pgvector, with filtered-recall tests and an exact fallback for bounded sets. Index entries contain record/version and embedding model identity; they are disposable projections. [U9, U12]

Combine candidates using a configured fusion/ranking policy with stable identity tie-breaks. Follow relevant support, conflict, derivation and procedure-condition links under traversal bounds. Exact equality and authoritative mappings take precedence over inferred entity matching. Contradictory candidates remain separately represented.

### 12.2 Freshness and index consistency

The domain collection is authoritative. Search results are rechecked for access, availability, current revision and scope before use. A mutation transaction creates an index-update event; projection workers record their watermark. For read-your-writes, merge a bounded set of relevant changes after the index watermark or use a direct scoped query. Never treat a stale projection as permission to expose deleted or revoked content.

Pagination carries stable cursor and query scope. Exhausted traversal, approximate-index filtering and inaccessible material are reflected in coverage. Candidate pruning that could remove necessary exceptions is evaluated and retains an inspection path.

### 12.3 Semantic selection

Classify evidence roles and assess procedure relevance, known prerequisites and exceptions with established checks. Full method instructions and required definitions are loaded for shortlisted candidates when a description is insufficient. A missing prerequisite can initiate a scoped investigation. Relevant conflicts return as a group including both sides, their support and open status.

Return the `ContextPackage` to the renderer. Record eligible, retrieved and selected sets separately. The renderer decides what physically enters context. Due intentions are routed through §15's atomic firing transaction, not inferred from a search hit. Existing workspace context may be sufficient; an empty addition is an explicit supported activation outcome.

### 12.4 Completion and tests

Measure applicable coverage, scope precision, conflict-group completeness and budgeted context quality. Unit tests exercise filters, stale indexes, time queries, cursor bounds and source identity. Semantic tests allow alternative sufficient context groups. Fixed-checkpoint continuations compare decisions under candidate, baseline and reviewed activation outputs. Cross-backend tests require semantic and access parity, not identical numeric relevance scores from different search engines.

## 13. Consolidation and procedure implementation

**Owner:** consolidation module. **Primary artifacts:** summaries, conditional claims, candidate procedures, proposed scope extensions and recognition criteria. **Triggers:** related completed task groups, recurring patterns, explicit teaching and scheduled synthesis. [C §§5–6; Appendix A]

### 13.1 Selection and synthesis

Select an explicitly bounded cohort by purpose, mechanism, entities, procedure, outcome and source context. Preserve cohort membership, evidence cutoff and the rules that selected it. Include known exceptions and independently sourced cases; group common-source restatements before evaluating evidential breadth.

Use structured comparisons for explicit conditions and scoped workers for synthesis. Jev can assess candidate pair relationships, possible counterexamples and clause-level preservation. A proposed abstraction states what varies, what stays invariant, which conditions support reuse and what remains untested. Candidate records remain usable as clearly labelled hypotheses when policy permits; established procedural use follows qualification.

### 13.2 Executable and advisory procedures

Executable procedures contain a governed artifact entrypoint, parameter contract, expected inputs/outputs, required sandbox/tool capabilities, effects, replay class, checks and supported conditions. Run them through Harbor/ASP with the same effect and resource policy as other code. Qualification does not confer host privileges.

Advisory playbooks contain steps, decision points, evidence requirements and stopping/verification criteria. Their assessment checks actual investigative choices and supporting results. Both forms retain examples, counterexamples, revision and applicability. A readable procedure label and stable ID support invocation; the definition loaded into a task is versioned.

### 13.3 Evaluation and adoption

Create a qualification job with held-out tasks, baseline conditions, resource limits and explicit acceptance criteria. Compare original episodes, concise summary and proposed reusable structure. Split related source cases and their variants together. A method is qualified only for the tested conditions; the resulting record identifies that scope and evaluator/profile versions.

The evaluation service produces evidence. A policy-governed adoption command changes the procedure's qualification state. No routine human approval is required when the automated acceptance policy is satisfied. Failed or inconclusive transfer tests preserve the candidate and its limits or retire it according to retention purpose.

### 13.4 Recognition criteria and repeated learning

A task-local check has purpose, inputs, question, lifetime and investigative role. Repeated usefulness can produce a consolidation candidate attached to a method. To establish a reusable check, evaluate packet quality, answer quality, routing outcomes and false-negative coverage. Adopt question, model and policy versions separately. Revisions that broaden the method or check's permitted use require corresponding qualification evidence.

### 13.5 Completion and tests

Results report cohort coverage, retained abstractions, preserved conditions, qualification status and cost. Tests include common-source duplication, a minority counterexample, overbroad scope, unjustified causal interpretation, exact reprocessing and safe deferral. Longitudinal evaluation includes the cost of synthesis and later maintenance when measuring amortised benefit.

## 14. Maintenance, changes and support implementation

**Owner:** maintenance module. **Primary artifacts:** revised records/relationships, support assessments, lifecycle outcomes and scoped change events. **Triggers:** source changes, corrections, missing support, deadlines, retention requests and bounded scheduled review. [C §9]

### 14.1 Change classification

Resolve authoritative source identity, applicable revision and explicit dependency candidates first. Classify the change as representation-only, support change, correction of prior understanding, world change with a new valid interval, access/retention change, or unresolved disagreement. Interpretive classification can use Jev or a scoped worker; source authority and permissions come from explicit metadata and policy.

A committed change carries its old/new references, relevant scope/time and reason. Wording-only changes update representation history while preserving supported conclusions. World changes preserve prior valid applicability. A correction affecting the old conditions can establish a targeted retrospective-review obligation under policy.

### 14.2 Active work and indirect dependencies

Maintain direct record dependencies in the collection. Find additional candidate relationships through bounded entity/source/topic search followed by interpretation. Each inferred dependency records its basis and status. Route substantive relevant changes to active tasks through their workspace references and event cursor; resumption can perform equivalent checks when an event is no longer retained.

A receiving task assesses its actual revision/freshness requirements before changing unfinished work. It either keeps the conclusion, qualifies it or requests new evidence. Completed results retain their original scope and verification history. A global reverse index of every historical session is not a system requirement.

### 14.3 Disagreement lifecycle

A conflict links incompatible claims in overlapping scope and valid time, their support and an investigation intention where useful. Related-but-distinct claims remain distinct. The open conflict is visible during relevant activation. In-task questions that determine the next action receive bounded investigation; independent or future-facing questions can run separately.

Resolution records the examined evidence, interpretation, valid/transaction time and relationship changes. It can support one position, establish different scopes or retain genuine uncertainty. The original evidence remains accessible under retention policy. Unresolved is a first-class result, not a fallback label that silently implies completion.

### 14.4 Support reassessment and retirement

Support assessment records independent sources, uncertainty, known challenges and verification history rather than an unexplained scalar confidence. New evidence can increase or reduce support. Removal of a source causes distinct checks for governed derivative content and separately retainable claims. A claim with adequate remaining support stays available with updated basis; otherwise routine guidance is retired and a useful revalidation intention can be created.

Retirement, archival access and deletion are separate operations. A retirement changes eligibility for current guidance but preserves allowed historical access. Governed deletion is performed through §17's cross-store workflow and includes active/restored state.

### 14.5 Completion and tests

Maintenance reports changed and preserved records, intended/actual scope, events, unresolved work and cost. Tests assert both required changes and preservation of unaffected state. Include bitemporal late knowledge, relevant versus unrelated active tasks, completed revision-C output, source deletion with and without independent support, event duplication and races with a user correction. Use expected revisions and bounded retries to avoid lost updates.

## 15. Intention lifecycle, timers and completion

### 15.1 Definition and occurrence

An intention definition contains purpose, scoped owner, trigger, readiness, completion condition, expiry and communication policy. Recurring definitions create separately tracked occurrences. Each occurrence follows `pending → armed → fired → completed`, with `expired` or `cancelled` reachable from every non-terminal state. Formation creates pending, maintenance arms and records terminal outcomes, and activation records firing. [C §8]

Trigger types include time, new applicable source revision, dependency/result arrival and a supported semantic event match. Known identifiers and event predicates use structured rules. A semantic trigger is an established question with explicit inputs and routing policy; an insufficient match leaves the occurrence armed until another event or expiry.

### 15.2 Arming and firing

Arming requires the owner, trigger, completion specification, finite expiry/review horizon and readiness to be defined. Persist event cursors or subscriptions and any timer in the same transaction as arming. Evaluate events that arrived during setup from the retained cursor to avoid a registration gap.

For firing, atomically verify state/readiness/expiry, record the trigger occurrence, change to fired, create the execution job and append its outbox event. A unique occurrence key combines the intention definition, recurrence window where relevant and canonical trigger identity. Duplicate event delivery returns the same occurrence. Intention firing and job creation either both commit or neither does.

### 15.3 Completion and terminal races

Execution attempts attach to the fired occurrence. Host success supplies evidence, not completion authority. Maintenance checks the declared result predicate, required source scope and optional designated confirmation before recording completed. A retry remains attached to the occurrence; it does not create a duplicate commitment.

Use database serialisation to decide races among completion, cancellation and expiry. Default semantics are completion when checked completion commits before expiry. A deadline explicitly defined on external completion time may accept authenticated timely evidence under a configured delivery grace period; its rule is declared at creation. The first valid terminal transition remains terminal. Late evidence is attached without rewriting that state; renewed work gets a linked follow-up occurrence.

Cancellation records authority and reason, requests host stop, and reports in-flight effect disposition separately. Expiry is enforced by durable due timers and checked again at firing and completion. Longer-lived commitments use explicit renewal horizons. After local downtime, due events are reconciled against current state and expiry before work is scheduled.

### 15.4 User-visible commitments and tests

Expose purpose, owner, trigger, completion evidence, expiry, status and notification rules. Amendments are versioned; changes to a fired occurrence either preserve its defined work or explicitly cancel and replace it. Communication suppression changes notification delivery, not state recording.

State-machine tests enumerate every transition, duplicate trigger, setup race, retry, cancellation order, expiry order and late-result path with a controlled clock. Test memory state independently of job attempts and actual sandbox effects. An unanswered required approval remains unresolved and never supplies completion or authorisation.

## 16. Structured semantic judgement and Jev

### 16.1 Adapter contract and provider request

A `JudgementDefinition` contains readable ID, version, owning process, full question, input requirements, answer form, criteria, known applicability, permitted policy uses, evaluation references and fallback. A `JudgementPacket` records subject/frame, actual evidence contents, their source/version/coverage, missing material, definition version, disclosure scope, deadline and budget. A `SemanticAssessment` records provider status and typed answer. A separate `PolicyDecision` records what the system did with that assessment. [C Appendix A.1–A.5]

The concrete Jev adapter sends `POST /v1/systemone` with `state`, `model` and a named `questions` map. It maps `choice`, `noul` and `score` responses to the project union. The documented API returns answer types, requested/returned model information and token usage; Choice and Score include distributions/confidence. [U7–U8]

Populate question instructions with the full criterion: the question ID is only a tracing key. Send actual source excerpts or structured observations, not just memory IDs. Include scope, source revision and missing evidence. Visual questions use a permitted visual/native observation step whose result is explicitly attributed; text-only judgement never pretends to inspect an unseen image.

### 16.2 Validation, attempts and batching

Validate response status, question-key coverage, matching types, finite values, allowed alternatives, distribution normalisation within a declared numerical tolerance, legend consistency and answer/rubric shape. An invalid result is transport/contract failure, not an answer of insufficient evidence. Store the raw response under packet retention policy and a validated normalised assessment for consumers.

A Noul value is the probability of the defined proposition; it does not carry an independently invented confidence field. For Choice/Score, distribution concentration and domain-calibrated correctness remain distinct. Preserve a concentrated answer of insufficient evidence separately from ambiguity between alternatives. [U7–U8]

Batch independent questions only when they share permitted evidence/disclosure and do not consume one another's answers. Dependent work creates a subsequent packet. Track inconsistencies across related answers and use the family fallback. A packet's scope must be valid for every question, including irrelevant source material that still becomes visible to all questions.

Own retry policy in one place. Disable or account for SDK automatic retries. Authentication/validation failures prompt configuration or packet repair; documented retryable rate/overload failures use bounded backoff within the root deadline. Record each provider attempt and unknown billing separately. An unavailable Jev service routes to an authorised baseline, deeper investigation or explicit unresolved outcome. [U7]

### 16.3 Complete judgement-family catalogue

Every row below is an implementation deliverable: versioned definitions, packet builder, result validation, policy disposition, fallback, test fixtures and qualification report. A family may comprise several independent questions to preserve overlapping properties. Example outcomes describe the required semantics, not a fixed claim that Jev already achieves them.

| ID / owner | Assessed question | Required policy behaviour |
| --- | --- | --- |
| J01 Formation | Does the candidate preserve observation versus attributed statement, inference or simulation? | Retain the actual evidential status; revise wording or seek context. Trusted origin metadata is carried mechanically. |
| J02 Formation/shared | Does the evidence support, contradict, leave unresolved or fail to address this claim? | Preserve supported scope; qualify or investigate a gap; keep counterevidence. |
| J03 Formation | Is this a meaningful event boundary, correction or continuation? | Select an interpretable bounded episode and link necessary surrounding events. |
| J04 Formation | Is the lesson a local convention, temporary workaround, conditional method or untested generalisation? | Retain the contribution in the appropriate family/status and scope. |
| J05 Formation/intentions | Does wording express an explicit commitment, suggestion or hypothetical action? | Propose the corresponding intention or preserve the statement without inventing an obligation. |
| J06 Activation | Is a candidate direct evidence, background, exception, contradiction or unrelated material? | Build a supported context group and keep relevant opposing evidence recoverable. |
| J07 Activation | Which available methods address the present question? | Independently assess methods, shortlist and load fuller definitions; allow no additional method. |
| J08 Activation | Are each method's prerequisites and applicability conditions established? | Select, qualify or request targeted evidence for unresolved conditions. |
| J09 Activation/maintenance | Do candidate references denote the same entity under the supplied scope? | Propose a supported mapping or unresolved alternatives; explicit identity rules remain enforced. |
| J10 Activation/maintenance | Do compared properties represent the same definition or quantity? | Establish comparable meanings before deterministic conversion/comparison. |
| J11 Consolidation | Are cases comparable, materially different, counterexamples or possible duplicates? | Construct a cohort preserving mechanism and boundary differences; recorded lineage supplies known source dependence. |
| J12 Consolidation | Does a proposed clause preserve the conditions and uncertainty in its source cases? | Retain or repair the abstraction; scope extensions require qualification evidence. |
| J13 Maintenance | Is a change wording-only, substantive correction, new valid-time state or unresolved? | Preserve unaffected conclusions; revise relevant records and route scoped events. |
| J14 Maintenance | Are claims incompatible within the same applicable scope and time? | Record a candidate/accepted conflict with support and appropriately scheduled investigation. |
| J15 Maintenance | Does remaining independent evidence address a claim after support removal? | Update support, preserve separately justified claims or retire routine guidance. |
| J16 Maintenance | Could a scoped source change affect a candidate indirect dependency? | Investigate the candidate relationship within retained records/relevant unfinished work. |
| J17 Intentions | Does submitted evidence satisfy a semantic part of the completion condition? | Combine assessment with required receipts and rules; maintenance owns the transition. |
| J18 Scoped harness | Does a work brief contain the definitions, versions, scope and evidence needed? | Complete or narrow the brief before dependent work; request only material missing input. |
| J19 Workspace/harness | Does a summary or handoff preserve qualifications, conflicts and unfinished checks? | Repair the affected transformation or return explicit partial coverage. |
| J20 Scoped harness | Are child findings duplicates, related-but-distinct, conflicting or independent? | Aggregate while preserving differences and common-source lineage. |
| J21 Results | Does demonstrated work justify each requested/claimed coverage item? | Complete missing work or narrow the coverage claim; exact enumerated coverage uses code. |
| J22 Results | Do current artifacts express consistent scope, findings and evidential status? | Revise affected wording under ownership rules while preserving user edits. |
| J23 Adaptive work | Is a technically successful result semantically suspicious for the intended query? | Inspect scope, schema and source assumptions; retain genuine observations. |
| J24 Adaptive work | Is repetition a new investigation, justified retry or unchanged unsuccessful approach? | Continue useful work or redirect to a discriminating check. |
| J25 Adaptive work | Which proposed check addresses a specific unresolved explanation? | Rank permitted investigation options with explicit resource/obligation rules. |
| J26 Adaptive work | Which workflow assumptions are supported, challenged or still unknown? | Orient to the unfamiliar convention or keep the assumption provisional. |
| J27 User interaction | Is a contribution task-local, a persistent preference, a hypothetical or a proposed shared rule? | Route to the relevant policy with actual actor authority and scope. |
| J28 User interaction | Does missing information require user intent or can permitted evidence resolve it? | Investigate autonomously or prepare the specific required question. |
| J29 Scoped work | Does this result need stronger interpretation, new evidence or a required check? | Allocate permitted additional work without waiving established checks. |

### 16.4 Routing and qualification

Each family policy maps validated answer, input coverage, calibrated operating condition and consequence to a permitted action. Retrieval reordering, writing a candidate claim, changing a supported claim and invoking an external effect have separate acceptance conditions. Calibrate on held-out domain cases; do not ship a universal confidence threshold.

Each family has a fully functioning reference route, such as structured rules plus a generative investigation. Start Jev in shadow mode, measure both flagged and unflagged cases, then enable bounded autonomous uses that meet the declared error/cost criteria. Full product completion requires catalogue coverage, fallback and evidence of safe routing for every family, not an assertion that Jev wins every comparison. The service can use the reference route where qualification evidence does not yet justify Jev.

### 16.5 Task-local checks and reusable definitions

A worker-created check inherits scope, input disclosure, completion requirements and a finite lifetime. It initially supplies investigative observations. Its question cannot enlarge effect permissions. Consolidation can package repeated useful checks as candidate recognition criteria; §13's qualification process establishes their permitted policy role. Maintain definition, model and policy versions independently.

### 16.6 Judgement provenance and caching

Record the actual contents supplied through protected packet artifacts, original source identities, question/rubric, requested and returned model identity, answer and usage. Use packet references rather than copy sensitive content into unrestricted logs. Generated explanations are separately attributed inferences; the API assessment itself supplies no fabricated rationale.

An assessment cache key includes packet contents or verified content-version equivalence, definition version, provider/model release identity, scope and required freshness. The host may use a checksum for this internal cache comparison. An alias such as `jev-latest` without a stable release identity uses restricted/short-lived reuse and is marked unpinned in evaluation. A policy-only change can re-evaluate an existing answer; changed evidence requires reassessment. Revocation and deletion invalidate affected cache entries immediately.

## 17. Security, privacy, retention and erasure

### 17.1 Access and trust

The local profile binds to loopback/UDS and uses an installation-specific credential and operating-system permissions. Production uses authenticated users and service identities, tenant/project authorisation, scoped worker capabilities and protected interservice channels. Authorisation is applied at query, read, packet construction, model egress, artifact materialisation and effect execution. Scope filters precede retrieval scoring and user-visible metadata.

Source content, agent-generated text and retrieved procedures retain their data/evidence role. Instruction authority comes from the configured task/system policy. Sandbox code has isolated filesystem/process/network access. Generative and Jev providers receive only material permitted by disclosure policy. Secrets are resolved by trusted adapters, never inserted into work briefs or model-visible environment variables.

### 17.2 Retention classes

Define policy classes for transient stream fragments, workspace scratch, resumable session content, retained memory, source artifacts, evaluation fixtures and minimal operation receipts. A class states purpose, expiry, allowed backup lifetime and erasure method. Hypothetical exploration selects read and learning-retention policy independently. Expiry of a temporary result does not imply erasure of the external source; the user-facing control explains the governed surfaces.

Foreign provider retention is tracked in the provider integration record. Disclosure-restricted work uses only eligible providers or local processing. The system reports provider deletion acknowledgements or retention limits honestly; it does not claim to recall already disclosed data without a supported provider mechanism.

### 17.3 Revocation barrier

A deletion/access command first commits the new revocation epoch and denies affected reads, prompts, materialisation and reuse. In-flight workers are notified and required to reconcile; dispatch of new calls checks the current epoch. Stop affected operations when their in-memory context contains removed material. Requests already sent to external services are tracked for cancellation/retention handling but cannot be assumed unsent.

Maintain a bounded exposure registry for live workspaces, session/artifact retention containers and cached packets. It exists to enforce access and purge covered surfaces, not to invalidate every historical result. On uncertain content lineage, conservatively include the affected container in erasure scope.

### 17.4 Cross-store erasure workflow

1. Authorise the instruction and enumerate governed records, versions, sources, derivatives, relation payloads, search projections, packets, artifacts, sessions and backups. Commit immediate revocation and a durable deletion job.
2. Fence active owners, halt affected calls and reconcile outstanding effects. Mark restored/resumable sessions as blocked until cleaned.
3. Purge domain content and derived representations covered by the instruction; remove lexical/vector entries and caches. Reassess separately retainable claims using independent remaining support.
4. Replace an affected Pi session under exclusive administration. Build a fresh clean session from authorised surviving facts, goals, obligations and result references; preserve remaining evidential qualifications. Treat generated text derived from deleted content as covered unless a clean independent basis is established.
5. Reconcile old unfinished effects, close the old Pi operations and move allowed unfinished work to an explicit continuation job/session. Never recreate old tool calls as commands to execute again. Maintain a minimal mapping or support-removed marker only where the deletion policy permits it.
6. Delete the old session container, journals, snapshots, spill files and temporary copies through the backend's administration path. Local SQLite administration includes WAL/SHM and reclaimed storage verification; encrypted storage and key retirement can support whole-container purge.
7. Purge permitted object versions and replicas, and schedule expiry of backups according to the declared retention policy. Any restore applies the deletion registry before enabling access.
8. Verify each target and issue a report distinguishing denied access, live-store purge, remaining backup expiry and provider actions. Completion uses the required scope rather than a single optimistic flag.

Precise in-place rewrite of Pi's historical tree is not assumed available upstream. The complete implementation uses controlled copy-to-clean-session and deletion, preserving lawful surviving work through a new continuation. If a future supported rewrite is adopted, it must pass the same content, lineage and replay tests. [U2–U3]

### 17.5 Deletion and completed results

Removal of supporting evidence can reduce a separately retainable claim's support and trigger targeted review. It does not automatically erase unrelated completed outputs or reinterpret historical validity. Where an output itself contains governed content, it follows the deletion instruction. This distinction is part of both the purge planner and the user report. [C §9]

## 18. User and operator experience implementation

### 18.1 Three surfaces

Implement a small web client and equivalent CLI commands over the same API. **Inside the task**, show current goal, selected context, evidence, returned findings, uncertainty and open obligations. **Entity/project views** assemble current understanding, methods, history, conflicts and commitments. **Attention/controls** expose prepared decision requests, scope, disclosure, retention, budgets and notification preferences. [C §§3–6, 8–9, 11–12]

Use recorded state as the view source. A context panel shows what was selected and declared applied, not an invented account of hidden reasoning. A delegated finding shows its producing operation, scope, evidence and gaps. Exact historical session reconstruction is available only where the relevant session manifests are retained.

### 18.2 Required interactions

| Experience | Implementation path |
| --- | --- |
| Inspect task context and basis | Read render/use manifests; inspect source/result artifacts under current access. |
| Correct a distinction | Submit an attributed correction with target, intended scope and expected revision; maintenance records the accepted effect. |
| Explore entity/project knowledge | Query by entity/scope/time; assemble claims, episodes, procedures, intentions and coverage. |
| Teach from a demonstration | Select an episode/correction/counterexample; formation captures it and consolidation develops/evaluates a method. |
| Track a future commitment | Create/amend/renew/cancel intention through §15; expose evidence-backed status. |
| Review meaningful changes | Build a scoped brief of established changes, candidate lessons and open work, grouped by consequence. |
| Explore temporarily and transfer selectively | Separate read-context permissions from retention/promotion; preserve hypothetical status and transfer restrictions. |
| Supply a needed decision | Show the exact missing intent/evidence/authority, affected work, owner, deadline and fallback. |

### 18.3 Autonomy and notification

Routine work continues under policy. A decision request is created only when permitted investigation cannot supply required intent, evidence or authority. Link it to affected work and a prospective review horizon. Independent work proceeds; affected authorised actions wait or take their declared fallback. Silence remains an unanswered request.

Notifications use an outbox and idempotent delivery identity. Users can choose material changes, blockers or completion notifications. Suppression affects delivery, not the recorded intention or task status. Corrections make their scope and consequences inspectable; reversing a memory edit is distinct from reversing an external action.

### 18.4 Operator functions

Operators activate tested profiles/policies, configure disclosure and retention, inspect aggregate resource use, suspend a provider/family, drain workers, request backup/restore and run qualification suites. Shared changes require the relevant authority; personal preferences do not modify tenant-wide rules. Administrative actions are ordinary versioned commands with receipts and test coverage.

## 19. Testing, evaluation and observability

### 19.1 Shared fixture and test contract

A fixture contains initial domain records, Pi/workspace state where relevant, visible evidence cutoff, request/events, policy, scope, allowed resources, reference-world facts and expected changes/preservation properties. Protected future evidence and grading instructions remain outside worker capabilities. Equivalent semantic representations are allowed when they preserve required meaning. [C §12]

Unit/contract tests use deterministic provider responses, tool results and clocks. Semantic evaluations use live models and evidence-grounded narrow criteria. Behavioural evaluations fork controlled checkpoints and run held-out continuations. Record code/profile/model/policy/question/grader versions and the actual source identities used.

### 19.2 Required suites

| Suite | Coverage |
| --- | --- |
| T01 Contracts/domain | Generated schema parity, record references, scope/authority, bitemporal queries, relationship direction, migrations and backend parity. |
| T02 Coordination/effects | Durable admission, deduplicated events, lost acknowledgements, leases/fencing, root budgets, retries, cancellation and unknown effects. |
| T03 Pi persistence | Upstream conformance; custom entries; accept/drive mapping; all reachable open-operation recovery states; session replacement. |
| T04 Harbor/ASP | Provisioning lifecycle; file/binary/path/error parity; truncation; remote cancellation; credentials; no local fallback; cleanup. |
| T05 Workspace | Lifetime, origins, conflict preservation, bounded provider context, checkpoint restoration, cache-sensitive layout and selective revalidation. |
| T06 Scoped execution | Expansion, decomposition, aggregation, partial coverage, child budgets, parent/child lineage, interpreter loss and recovery. |
| T07 Formation | Pre/post-correction evidence, required retention, explicit preferences, hypothetical input, duplicate sources and local scope. |
| T08 Activation | Entity/time/access filters, applicable methods, opposing evidence, index lag, sufficient empty addition and budgeted context groups. |
| T09 Consolidation | Conditional abstraction, counterexamples, common-source support, candidate recognition criteria and held-out procedural transfer. |
| T10 Maintenance/intentions | World change versus correction, completed-result preservation, every lifecycle edge and event ordering, expiry and late evidence. |
| T11 Jev | All J01–J29 packet/assessment/policy boundaries, invalid replies, timeout, independent batching, calibration and fallback. |
| T12 Security/erasure | Cross-tenant isolation, disclosure, source-instruction attacks, revocation races, derived-content purge, sessions, caches and restore. |
| T13 User/autonomy | All eight interaction experiences, user absent, scoped correction, meaningful notification and unanswered authorisation. |
| T14 Deployment/operations | Multi-worker replacement, database outage/failover, backups, complete restore, upgrades, rollback and local-to-production migration. |
| T15 Whole-system value | Controlled substitutions, baseline comparisons, longitudinal learning, cost/quality, missed issues and induced errors. |

Tests ship with their corresponding implementation package. T15 is assembled from earlier fixtures; evaluation infrastructure is present from WP01 rather than added after implementation.

### 19.3 Evaluator reliability

Use exact assertions for state, identity, time, budget and effects. Semantic graders assess specific source-to-claim or procedure-to-case questions and cite their basis. Human-reviewed samples calibrate accepted/rejected cases and newly introduced domains; routine runtime operations continue under automated policy.

Test graders with paraphrase, consistent renaming, candidate order and meaning-preserving reorganisation. Deliberate changes to revision, uncertainty, source identity or counterexample coverage must alter the relevant judgement. Property-based tests exercise valid event sequences and invariant preservation. Changes to grading reprocess saved outputs; changed task instructions/evidence require fresh executions.

### 19.4 Experimental comparisons

Compare history-only access, curated memory and the complete system under matched task/evidence/resource conditions. Replace individual process outputs at fixed checkpoints with reviewed alternatives drawn from the same evidence cutoff. Compare structured direct processing, a focused worker and optional recursive execution. Compare baseline semantic checks, a conventional model with the same questions, and Jev with its routing policy.

Long-running studies reset independent arms to the same initial state and preserve learning within each declared history. Hold related project/source cases and their variants in one split. Score a task before admitting its outcome as feedback for future tasks. Report uncertainty by independent scenario family, not just by number of paraphrases.

### 19.5 Performance qualification

A versioned workload manifest declares record counts, artifact sizes, relation density, tenant distribution, active sessions, input rate and expected deadlines. Exercise at least small functional fixtures, medium search/load fixtures and a sustained workload at the declared production capacity. These are measured deployment conditions, not an assumed scale claim.

Reports include latency percentiles by operation class, queue age, database contention, index lag, token/cache usage, root-budget accounting, sandbox startup/cleanup and restore time. Semantic and behavioural reports include supported outcomes, missed obligations, scope mistakes, induced failures, false alarms and human effort. Define quality tolerances and cost/latency targets before comparing candidates; record achieved results rather than filling the spec with unmeasured promises.

### 19.6 Observability and data minimisation

Use stable job/session/operation/invocation/request IDs for correlation. Instrument API admission, DB commit, model/Jev attempts, render, sandbox execution, child dispatch, effect receipt, domain adoption and deletion. Distinguish live progress from durable settlement. Expose counts and timings by tenant/work class without placing source text in unrestricted telemetry.

Operational dashboards show owned sessions, pending/waiting jobs, stale leases, unknown effects, unresolved usage, failed purges and provider fallbacks. Alert on invariant violations and stuck work. Every alert has an operator action or runbook. Detailed evidence remains in governed artifacts and expires according to purpose.

## 20. Local and production deployments

### 20.1 Local profile

One supervised Rust host runs the API, scheduler and domain modules. It starts a bounded Node worker pool and the Harbor Python bridge. SQLite stores domain state; Pi's SQLite backend stores sessions; local directories store governed artifacts. Harbor creates isolated Docker execution environments. The local UI/CLI connects to the authenticated local API; remote model and Jev calls obey disclosure policy.

Use WAL, `synchronous=FULL`, foreign-key checks, a bounded busy timeout and short write transactions for important local state. Verify the runtime SQLite version and required extensions through the compatibility manifest. WAL supports local readers with one writer and is not a network-filesystem multi-host store. [U9]

A local service can restart under the operating system supervisor. At startup it verifies migrations, obtains ownership, scans pending jobs and open Pi operations, reconciles expired intentions and orphan sandboxes, and then accepts new work. A stopped machine preserves work but executes nothing until restarted. Local backup uses consistent SQLite snapshots and an artifact manifest. [U13]

### 20.2 Production profile

Deploy the API/coordinator with PostgreSQL domain storage, object storage, trusted Pi workers, the PostgreSQL Pi Session backend, a Harbor bridge and the qualified SSH-capable sandbox provider. Secrets come from the deployment secret system. Workers receive scoped credentials/leases and connect to model/Jev services through trusted adapters. The application defines separate domain and session database roles even if they share a cluster.

Separate interactive activation/investigation capacity from deferred formation, consolidation and maintenance capacity. Apply per-tenant quotas and fair work claiming. Every long job has bounded units so one tenant cannot monopolise all workers. Add capacity by work class; adjust concurrency using measured provider, database and sandbox limits. Database-backed delivery is the selected complete design, not a placeholder for a mandatory broker.

Read models and caches are disposable. Domain correctness uses the primary database or a causally adequate read after a known commit. Event/outbox consumers maintain cursors and replay safely. Vector/lexical projections may lag but final access/version filtering is current. Use production Session storage fencing before permitting more than one host to claim sessions.

### 20.3 Backup and disaster recovery

Database durability, worker recovery and disaster recovery are separate checks. Configure PostgreSQL base backup/WAL archiving or an equivalent managed recovery profile, including domain and session schemas; object storage uses compatible version/retention rules. PostgreSQL documents recovery through base backups and archived WAL. [U13]

A backup manifest records database recovery position, artifact inventory/version policy, worker/session compatibility, policy/profile definitions and a deletion-registry watermark. Restore into isolation, apply outstanding deletions and access changes, verify referenced artifacts, reconcile effects and only then enable workers. When external effects may have happened after the restored database point, query their owners/receipts before resuming those operations.

Record measured recovery point and recovery time against the deployment's declared target. Test a complete restore, not only that backup files exist. Restore tests include open sessions, due intentions, deleted source material and an unknown external effect.

### 20.4 Migration from local to production

Drain new local writes, pause active drivers at supported boundaries, reconcile unknown effects and produce a consistent export of domain records, session repositories, artifacts, policies and receipts. Import into production under unchanged identities and explicit scope mapping. Migrate Pi data through the pinned Session/storage export or adapter conversion under exclusive ownership; do not translate arbitrary private state without a tested mapping.

Rebuild projections, compare record/version counts and representative temporal queries, verify artifacts and test continuation on a copy. Activate a single production ownership epoch, redirect clients, and retain the local snapshot offline only for the permitted rollback window. Prevent both environments from driving the same live session or intention. After activation, rollback follows explicit migration/reconciliation rules rather than starting the old copy.

### 20.5 Upgrades and incident runbooks

Deliver runbooks for worker crash, provider outage, sandbox disconnect, source revocation, stuck intention, unknown effect, database failover, index corruption, budget mismatch, failed purge and rollback. Pin model/question/profile identity in reports. Drain sessions incompatible with a worker upgrade; canary a new build against fixtures and selected permitted workload before wider assignment. Administrative migration and deletion take the same exclusive ownership path as recovery.

## 21. End-to-end acceptance stories

### 21.1 Property investigation, reuse and revision

Run the C/D scenario from the conceptual specification with concrete model/tabular fixtures. Ingest an empty instance query, a schema inspection and a verified type-level result. Form an episode and C-scoped claim. Consolidate an advisory inspection method with an exception. Create and arm a recheck intention. Activate the method for D; run a scoped Harbor/ASP investigation with a Jev or baseline applicability check; return a bounded result, commit D's evidence and complete the intention through checked conditions.

During the story, interrupt a Pi worker after a settled model response and after a domain commit whose receipt is lost. Resume without duplicating the domain change or treating partial progress as a verified result. Compact the main context and preserve source versions, conflict status and obligations. The completed C report remains correct within its recorded scope. Run the story locally and on the production profile.

### 21.2 Autonomous learning and human influence

Run a series of related tasks while the user is absent. Routine formation/consolidation/upkeep proceed within budgets. A user later inspects an entity, corrects a reporting preference in a narrow scope, teaches a method with a counterexample, creates a commitment and opens a temporary hypothetical branch. The correction applies only in its intended scope; the branch's assumptions remain temporary; the method's qualification is based on tests. A required authority question remains outstanding with an owner/fallback while independent work continues.

### 21.3 Deletion, recovery and historical scope

Delete an episode containing governed content that appears in a packet, search projection, generated summary and active Pi session. Immediate access changes block new use. Purge or replace covered derivatives and sessions, reassess a separately supported claim, and report backup expiry honestly. Restore a backup in isolation and demonstrate that removed content stays inaccessible. Surviving work resumes in a clean continuation with its verified remaining evidence and resolved effect history.

### 21.4 Complete-release criteria

A complete implementation passes every applicable deterministic fixture and backend conformance test with no unexplained failures; provides a report for every semantic family and its active fallback; meets the deployment's predeclared quality/resource requirements; and has demonstrated both local restart and production worker replacement/restore. Each conceptual requirement in §23 has an owner, implementation location, test and delivered artifact.

Catalogue coverage and safe fallback are required for every Jev family. A particular provider's qualification can remain limited to its demonstrated domain while the full process behaviour is delivered through the reference route. Experimental ASP/Pi dependencies have pinned compatibility evidence and failure handling. No requirement is declared complete merely because an interface, mock or diagram exists.

## 22. Dependency-ordered implementation plan

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
| WP14 User and operator surfaces | WP06, WP09–13 | Complete interaction API/CLI/UI and autonomous notification/decision handling. |
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

**Implement:** all eight experiences in §18 through one API/CLI/web contract; task evidence and context inspection, corrections, teaching, entity/history views, commitments, change briefs, temporary branches and prepared authority requests. Add policy/provider controls and notification delivery.

**Exit evidence:** unattended workflows continue appropriately; one scoped correction changes the intended future work; notification suppression preserves state; users see the basis/coverage actually recorded; silence remains unresolved. **Tests:** T13 and applicable authority/retention cases from T12.

### 22.16 WP15 — Complete deployment and operational recovery

**Implement:** supervised local installation and production manifests; databases/object storage, trusted worker pools, Harbor service, secrets, tenant fairness, monitoring, backup/restore and local-to-production migration. Write and exercise every runbook in §20. Qualify the actual provider/image/runtime matrix.

**Exit evidence:** restart, worker replacement, database failover, complete restore and ownership transfer work with live operation state; deployment load reports state achieved bounds; teardown and retention jobs recover after outages. **Tests:** T14 with whole-stack fault injection.

### 22.17 WP16 — Whole-system qualification and release

**Implement:** assemble all conceptual scenarios and acceptance stories; run process substitutions, long histories, cache/result-reuse studies, all-family judgement qualification, user-effort tests and scoped-execution ablations. Freeze the supported version matrix and publish operational/semantic/behavioural results separately.

**Exit evidence:** §21's complete-release criteria; every traceability row resolves to delivered code, tests and evidence; the release specifies each Jev family's active mode and fallback; local and production profiles pass the same functional stories. No component's mock-only test is presented as a live integration result. **Tests:** T01–T15 and the release manifest.

## 23. Conceptual coverage and completion control

The companion `agent_memory_implementation_traceability_v1.0.md` maps every behavioural/design subsection of the attached source to implementation sections, work packages and test suites. Source rationale/reference sections are retained as provenance rather than treated as new runtime components.

| Conceptual requirement | Implementation | Delivery and evidence |
| --- | --- | --- |
| Two stores, four peer processes, shared policy | §§1–4, 10–14 | WP01–04, WP09–12; T01–T03, T07–T10. |
| Working versus retained memory; all families | §§3, 8, 11–15 | WP02, WP06, WP09–12; family/schema tests. |
| Origin, lineage, entity identity and source access | §§3–4, 8–9 | WP02, WP06–07; T01, T05–T06. |
| Bitemporal records and relationship vocabulary | §3, §14 | WP02, WP12; temporal/conflict tests. |
| Bounded rendering, compaction and cache awareness | §8 | WP06; T05 and provider-payload/runtime experiments. |
| Scoped work, decomposition and referenced material | §§6–9 | WP04–07; T03–T06. |
| Complete process input/output behaviour | §§10–14 | WP09–12; process suites and continuations. |
| Autonomous policy, budgets and observable use | §§5, 10, 16, 19 | WP03, WP08; T02, T11, T15. |
| Full intention lifecycle, expiry and recurrence | §15 | WP12; T10 and event-order tests. |
| Selective revalidation and completed-result preservation | §§8, 14, 21 | WP06, WP12; T05, T10, T15. |
| Deletion including derived content and sessions | §17 | WP13; T12, restore tests. |
| User direction, all interaction experiences | §18 | WP14; T13 and unattended runs. |
| All conceptual test levels and scenarios | §§19, 21 | Tests from WP01 onward; WP16 release evidence. |
| Appendix A complete judgement integration | §16, §§5/9/19 | WP08 plus every process; J01–J29 and T11. |
| Harbor/ASP and Pi architecture agreement | §§2, 5–7 | WP01, WP04–05; conformance and recovery. |
| Local and production system end states | §20 | WP02–05, WP15–16; backend parity and restore. |

A work package is complete when its contracts, live integration, required failure behaviour and tests are delivered. A process or provider can have limited semantic qualification while its complete reference behaviour and safe fallback are implemented. Gaps in scope, experimental dependencies and measured performance remain visible in the release record.

## Appendix A. Concrete application contracts

The following names and bindings are proposed project interfaces. They translate the responsibilities above into implementation work; they are not claims about existing Pi or Harbor APIs. Generate Rust/TypeScript clients and server validation from their shared schemas.

### A.1 Core payloads

| Payload | Required fields beyond the common command envelope |
| --- | --- |
| `WorkBrief` | Purpose; process; task/frame references; source and memory versions; allowed source/query/tool capabilities; known conflicts; scope/time/freshness; output criteria; root budget; child limits; retention and disclosure policy. |
| `ContextPackage` | Query and evidence cutoff; eligible/retrieved candidates; grouped claims/support/exceptions; applicability states; due obligations; deferred material; source/record versions; projection watermark; coverage and reason. |
| `WorkResult` | Complete/partial/blocked status; examined scope; findings; actual source/record inputs; supporting/challenging evidence; applicability; unresolved work; proposed changes; child outputs; result artifact; known effects; usage status. |
| `ChangeProposal` | Process and purpose; target IDs and expected revisions; new typed representations/relationships; source evidence; valid-time changes; evidential and availability states; requested effects; preservation requirements. |
| `MutationReceipt` | Logical command ID; request-equivalence key; accepted actor/scope; commit sequence/time; resulting versions/events; outcome/effect references; policy version. |
| `JudgementPacket` | Definition/question version; subject and frame; actual evidence contents/references; missing material; coverage; disclosure permissions; provider/model request; deadline; budget and consuming policy. |
| `Assessment` | Packet/definition identity; valid/invalid/service status; returned model; typed answer/distribution if supplied; raw response reference; provider attempt and usage. |
| `RenderManifest` | Workspace/session/lane and decision ID; actual content groups/references/versions; origins; conflicts/obligations preserved; tool schema/profile; estimated/final usage; deferred content; stable-prefix/rebuild strategy. |
| `MemoryChangeEvent` | Event ID and cursor; commit sequence; actor/process; affected records and prior/current versions; scope and valid interval; semantic change category; evidence/result references; policy version. |
| `EffectReceipt` | Stable effect key; canonical request reference; permitted scope/versions; attempt IDs; prepared/in-progress/known/unknown state; external operation identity; observed outcome and reconciliation evidence. |

All payloads have explicit schema versions. Dates use UTC instants with declared valid-time interpretation; numeric identifiers/counters use ranges that round-trip between Rust and JavaScript. Monetary usage uses an integer minor unit or decimal representation with currency, not a binary floating-point account balance.

### A.2 Proposed HTTP bindings

| Method and path | Behaviour |
| --- | --- |
| `POST /v1/tasks` | Create an idempotent task, workspace and initial job; return durable task/job IDs. |
| `POST /v1/tasks/{id}/inputs` | Add attributed input under task authority; distinguish a scope correction from a normal message. |
| `GET /v1/tasks/{id}` | Task status, current work, unresolved requests and result references. |
| `GET /v1/tasks/{id}/context` | Recorded selected context and declared-use view under current access. |
| `POST /v1/tasks/{id}/cancel` | Request durable cancellation and return its current reconciliation status. |
| `POST /v1/tasks/{id}/branches` | Create a branch with explicit context/retention policy. |
| `POST /v1/memory/query` | Bounded scope/entity/time/family query with cursor and coverage. |
| `GET /v1/memory/{id}/versions/{revision}` | Read the named permitted representation and support links. |
| `POST /v1/memory/contributions` | Submit an attributed candidate contribution; return its formation job. |
| `POST /v1/memory/corrections` | Submit a target, distinction, evidence, scope and expected revision. |
| `POST /v1/processes/{kind}/runs` | Schedule a process with its work brief; the kind is allowlisted. |
| `POST /v1/intentions` | Create a pending definition/occurrence with expiry and policy. |
| `POST /v1/intentions/{id}/commands` | Authorised amend, renew or cancel commands with expected state/version. |
| `GET /v1/jobs/{id}` | Status, attempts, wait conditions, checked result and known effects. |
| `GET /v1/events` | Authorised event stream/catch-up cursor; gaps require a resnapshot. |
| `POST /v1/artifacts` | Allocate a governed upload; finalisation/read bindings use scoped references. |
| `POST /v1/admin/deletions` | Start immediate revocation and a durable cross-store purge. |
| `GET /v1/admin/deletions/{id}` | Per-surface completion, outstanding retention/provider work and evidence. |

Worker-control bindings live on an authenticated internal service surface and expose claim/renew, session admission mapping, budget permits, provider usage, effect receipts and result publication. User tokens cannot call those endpoints. Mutations are accepted with `202` when asynchronous; completed reads/commands use normal success responses. Typed `409` conflicts identify stale expected versions or reused command IDs with different content. Authentication, forbidden scope, unavailable service and invalid payload have distinct responses. Transport failures do not erase accepted operations.

### A.3 Required configuration records

A complete deployment supplies a compatibility manifest, model/disclosure profiles, sandbox capability profiles, memory policies, question definitions/routing policies, retention classes, resource envelopes and evaluation manifests. There are no production secrets in these versioned records. Providers and secrets are referenced by protected identifiers.

Each resource envelope declares finite values for provider attempts, request deadline, child depth/concurrency, artifact/output size and operation lifetime, plus a root currency/token allowance where applicable. Each deployment declares lease/heartbeat timing, queue fairness, backup retention, restore objectives and supported workload. Defaults are versioned and tested for the chosen environment. Quality/calibration thresholds are established on development data and frozen before held-out evaluation; an absent threshold selects the defined reference route rather than implicitly accepting a judgement.

### A.4 Transaction examples

**Retain a corrected claim:** verify actor/epoch and expected revision; read required support; insert the new temporal representation and source links; record the policy decision and command receipt; append the scoped change event. Commit all domain writes together. Index processing occurs afterward and cannot weaken current access checks.

**Fire an intention:** lock its eligible occurrence; verify trigger, readiness and expiry; record canonical trigger identity; mark fired; create the host execution job and outbox event. Duplicate trigger identity returns the existing occurrence/job.

**Consume a process result:** validate result-artifact readiness and declared source versions; assess relevant intervening changes; apply the permitted proposal or create bounded follow-up work; record receipt and output status. A changed input can require targeted refresh while preserving unaffected results.

**Resume a Pi operation after a lost domain response:** recover the same invocation identity from Pi; query the memory service's receipt; return the existing committed result when request equivalence matches. If no receipt exists, follow the effect's prepare/reconciliation rule before retrying.

## Appendix B. Source baseline and implementation evidence

### B.1 Source-derived requirements

**C — Attached conceptual specification:** `specification.md`, “Agent Memory — System specification”, 17 September 2026. The document supplied for this task is the requirements baseline. It includes the agreed scope, RLM-style work, user interaction, testing and Appendix A. Earlier chat drafts and earlier v0.1/v0.2 artifacts are background; they do not override this attached source.

All newly introduced module names, API operations, schema layouts, job states, transport choices, defaults and work-package ordering are proposed implementation decisions. Their acceptance evidence is produced by implementation and tests, not by this document.

### B.2 Checked upstream sources

**U1 — Pi public harness API, pinned source.** `packages/agent/src/harness/agent-harness.ts` at `e4c75a73222ae2c72abb5f5314fa35ee8effc508`. Basis for create/accept/drive, lanes, hooks, context projection and observed result types. [Source](https://github.com/earendil-works/pi/blob/e4c75a73222ae2c72abb5f5314fa35ee8effc508/packages/agent/src/harness/agent-harness.ts).

**U2 — Pi harness implementation specification.** `packages/agent/docs/harness.md` at the same commit. Basis for session-owned state, atomic transitions, intent/settlement, single writable owner, tool replay and pre-stabilisation storage. [Source](https://github.com/earendil-works/pi/blob/e4c75a73222ae2c72abb5f5314fa35ee8effc508/packages/agent/docs/harness.md).

**U3 — Pi roadmap/audit.** `packages/agent/docs/post-wp05-roadmap.md` at the same commit. An upstream audit inventory identifying gaps and conflicting future contracts; it is not proof that every described future capability ships. Used to define explicit project closure and tests. [Source](https://github.com/earendil-works/pi/blob/e4c75a73222ae2c72abb5f5314fa35ee8effc508/packages/agent/docs/post-wp05-roadmap.md).

**U4 — Pi harness tool and execution types.** `packages/agent/src/harness/types.ts` at the same commit. Basis for invocation identity/memos, ExecutionEnv, filesystem errors, paths and scoped tool context. The package README identifies the separate SQLite Session backend. [Types](https://github.com/earendil-works/pi/blob/e4c75a73222ae2c72abb5f5314fa35ee8effc508/packages/agent/src/harness/types.ts) · [README](https://github.com/earendil-works/pi/blob/e4c75a73222ae2c72abb5f5314fa35ee8effc508/packages/agent/README.md).

**U5 — Harbor ASP RFC #3023.** Open/unmerged when checked on 17 September 2026; head `dd4784b1ade1f446399e194f6dffd15142a77b98`. Basis for `.asp.json`, SSH transport, separation from provisioning, fresh-shell/persistent-filesystem semantics and reference provider experiments. [Pull request](https://github.com/harbor-framework/harbor/pull/3023).

**U6 — Harbor custom sandbox contract.** Official documentation describes the environment lifecycle, command execution and file transfer interface. Actual provider release and capabilities are pinned and verified during WP01/WP05. [Documentation](https://docs.harborframework.com/core-concepts/sandboxes/custom-sandboxes).

**U7 — TypeSafe API.** Official request/response, model/usage and error documentation, checked 17 September 2026. [API](https://docs.typesafe.ai/api).

**U8 — TypeSafe question/confidence semantics.** Complete instructions, independent questions over shared state and distribution-derived confidence. [Primitives](https://docs.typesafe.ai/primitives) · [Confidence](https://docs.typesafe.ai/confidence).

**U9 — SQLite operation and full-text search.** WAL's local-host concurrency model, synchronous settings and FTS5. [WAL](https://sqlite.org/wal.html) · [PRAGMA](https://sqlite.org/pragma.html) · [FTS5](https://sqlite.org/fts5.html).

**U10 — PostgreSQL work claiming.** Official SELECT/locking documentation, including queue-like use of `SKIP LOCKED`. [Documentation](https://www.postgresql.org/docs/current/sql-select.html).

**U11 — Pi inspected revision.** Commit metadata fixes the source snapshot for the above review; the commit is dated 16 September 2026 UTC. It is not a claim that a particular package release has been installed or integration-tested. [Commit](https://github.com/earendil-works/pi/commit/e4c75a73222ae2c72abb5f5314fa35ee8effc508).

**U12 — pgvector.** Project documentation for exact/approximate vector search, filtering and index behaviour. The project selects and qualifies an extension version during the production build. [Repository](https://github.com/pgvector/pgvector).

**U13 — Backup/recovery documentation.** SQLite online backup and PostgreSQL base-backup/WAL recovery supply mechanisms for the proposed coordinated restore procedure. [SQLite backup](https://sqlite.org/backup.html) · [PostgreSQL PITR](https://www.postgresql.org/docs/current/continuous-archiving.html).

### B.3 Compatibility evidence to produce

The final build manifest must replace source-only assumptions with tested package versions and commands, chosen Harbor/provider versions, ASP schema/reference pin, image identities, runtime/SSH versions, database/extension versions, provider model identities and actual conformance results. It also records any narrow upstream patches for required integration behaviour, their tests and the update strategy. No execution, benchmark result or supported production capacity is claimed by this specification itself.
