# Release status

This states what the memory architecture implementation qualifies for use today, and what it
does not, honestly and separately for operational correctness, semantic quality and behavioural
value (see [the runtime guide](docs/runtime.md) for how the system works).

## 1. Version matrix

| Component | Version / revision | Evidence |
| --- | --- | --- |
| Rust toolchain | Cargo.lock-pinned | `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test` |
| Node / npm | `engines: >=22.19` | `package-lock.json`; vitest |
| SQLite | via `node:sqlite` and the sqlx bundled driver | store, coordinator and Pi backend suites |
| PostgreSQL | disposable cluster in `npm run test:store` | store parity, Pi backend, migration |
| Pi (harness, ai, chord, telemetry, sqlite backend) | pinned upstream commit, built from source | upstream conformance suite |
| Harbor | pinned reference (`services/harbor-bridge`) | `npm run test:docker` (Docker lifecycle), bounded Daytona profile |
| ASP | pinned reference (`npm run test:asp`) | loopback OpenSSH |
| Python | bridge, importer, store tests | bridge test suite |
| Embeddings | Nomic Embed Text v1.5, local, CPU (Transformers.js) | `evals/activation/nomic-smoke.json` |

Deployment profiles: **local** (single Host, SQLite, Host-spawned Node pool, Docker sandbox) is
implemented and exercised by the test suites above. **Production** (`deploy/`: PostgreSQL with
separate roles, file secrets, TLS proxy, Host + pool image) is written and syntax-validated; it
has not been built or run in this repository.

## 2. Judgement families: evaluation status

`packages/judgement/src/policy.ts` defines the runtime's own evaluation status, not a document's
claim about it: a family/definition-revision/model-release/scope combination is **`qualified`**
only when an operator supplies a `Qualification` — a held-out evaluation artifact and a
selected-option probability threshold — that covers it and every answer meets that threshold.
Absent a matching qualification, the status is **`experimental`**: the family's judgement route
still runs and its owning process still uses its answer, but nothing certifies that answer
against held-out evidence.

**No `Qualification` is configured anywhere in this repository — not in `worker.json`, not in
`main.ts`, not in any deployment manifest.** Every one of the 31 catalogue families is therefore
`experimental` today, for every model release. `npm run inventory` lists every family with its
owner, permitted uses and whether an eval fixture exists for it — a weaker signal than
qualification (a fixture exercises the contract; only a held-out gate qualifies an answer). All
31 families currently have an eval fixture; none has a held-out qualification gate.

Every family's fallback, as defined in the catalogue, is the same: use the configured
`fallback` route with the same evidence and questions, or return unresolved work for the owning
process. The runtime implements this — a failed or unparseable reply is recorded as an
`unavailable`/`invalid_response` assessment and the owning process defers the item rather than
inventing an answer (exercised by the provider-outage fault test).

| Families | Owner | Live evidence | Status |
| --- | --- | --- | --- |
| J01, J02, J03, J04, J05, J30 | Formation | `evals/formation/live/README.md` (a live run with the pinned generative route; malformed replies before strict schemas were tightened); `evals/judgement/live-smoke*.json` (batching agreement) | experimental |
| J06–J10 | Activation / maintenance | scripted contract tests; `evals/activation/nomic-smoke.json` (two paraphrase retrieval cases) | experimental |
| J11, J12 | Consolidation | `evals/consolidation/live-2026-09-18.json` (a live synthesis run; the judge accepted an unsupported generalisation — a real negative result, not a pass) | experimental |
| J13–J17 | Maintenance / intentions | scripted contract tests | experimental |
| J18–J20 | Scoped harness / workspace | scripted contract tests | experimental |
| J21, J22 | Results | scripted contract tests | experimental |
| J23–J26 | Adaptive work | scripted contract tests only; no runtime process dispatches them yet | experimental |
| J27, J28 | User interaction | scripted contract tests | experimental |
| J29 | Scoped work | scripted contract tests | experimental |
| J31 | Investigation (decision) | scripted contract tests | experimental |

Qualifying a family requires held-out positive, negative, missing-evidence, ambiguous and
conflicting cases per family and domain, compared against the configured fallback route, with a
threshold chosen from those results (see `evals/judgement/README.md`). None has been produced yet.

## 3. Results, reported separately

### 3.1 Operational (deterministic fixtures and backend conformance)

Gates: `npm run check` and `npm run test:store`. Coverage: Rust contracts, store (SQLite and
PostgreSQL), Host HTTP, authentication, runtime credentials, pool claiming and fairness, child
supervision, backup, operations endpoints; TypeScript Pi worker, workspace, judgement contract,
formation/activation/consolidation/qualification/maintenance/decision runtimes, the pool
composition matrix, the headless continuous scenario against the real production entrypoint
(source-backed task → automatic formation → retrieval → a mid-job kill and pool restart →
recovery), CLI walkthrough, backup/restore; Python bridge and ATIF importer; upstream Pi
conformance. Fault injection exercised: worker crash after claim, judgement provider outage, Host
restart with pool restart, stale granted input, expired lease recovery, lost commit receipt,
Host failure before/after commit.

### 3.2 Semantic (live model behaviour)

| Report | What it shows | What it does not show |
| --- | --- | --- |
| `evals/formation/live/README.md` | Formation pipeline runs live against a real generative provider | Qualification of any family |
| `evals/judgement/live-smoke*.json` | Batched and separate-family calls agree; a fixed criteria ambiguity | Held-out accuracy |
| `evals/consolidation/live-2026-09-18.json` | Live synthesis and advisory transfer run; the judge accepted an unsupported generalisation | Consolidation retention quality |
| `evals/activation/nomic-smoke.json` | Two paraphrase cases retrieve correctly with local embeddings | Retrieval usefulness at scale |
| `evals/live-walkthrough/run-2026-09-18.json` | A model works a task from corrected memory; findings judged and retained with provenance; pool-driven with a Host restart | Model or judge quality; n is tiny |
| `evals/campaign/` (dry run only; see `evals/campaign/README.md`) | The campaign scaffold's arm isolation, transfer holdout and cost reporting over three fixture tasks | Any live campaign result — the fixture runner has no live execution driver |

### 3.3 Behavioural (acceptance stories)

| Story | Status |
| --- | --- |
| Property investigation, reuse and revision — an agent solves a task, forms an episode/claim/method, consolidates, reuses the method on a later task, an intention completes | **Exercised as one unattended story.** `packages/supervisor/test/learning.test.ts` waits for the Host's own scheduler to carry formation → cohort consolidation → qualification forward, then adoption, later activation retrieval of the adopted method, a correction that schedules maintenance, and an intention firing and settling — nothing in that path is hand-submitted. |
| Autonomous learning and human influence | **Exercised.** The user-guide walkthrough covers scoped correction, teaching with counterexample, commitment, temporary exploration, an outstanding authority question while other work continues; `continuous.test.ts` and the scheduler tests cover the pool carrying formation, consolidation, qualification and maintenance forward while unattended. |
| Deletion, recovery and historical scope | **Exercised at component level**, not yet as one continuous story: packet/projection/summary/session copies, access blocked before purge, surviving supported claims, honest backup expiry; the restore test restores in isolation and keeps deleted content inaccessible via the registry; clean continuation is tested. |
| Composition and shutdown | **Exercised.** `compositions.test.ts` composes and runs a job through every supported composition (a lone process module, judgement without memory processes, read-only memory, capture-only, full learning, full learning minus one module), asserting that a missing required service is rejected before any claim, a disabled module leaves data and scheduled work intact, and shutdown drains or cancels outstanding work within its configured window. |

## 4. Release criteria — status

| Criterion | Status |
| --- | --- |
| Every applicable deterministic fixture and backend conformance test passes with no unexplained failures | Met for the local profile; PostgreSQL parity met under a disposable cluster |
| A report for every semantic family and its active fallback | Met (§2): all 31 families `experimental` with the catalogue fallback; live evidence where it exists |
| Predeclared quality/resource requirements | Resource bounds met (budgets, reservations, payload limits); **semantic quality targets not declared or met** |
| Local restart and production worker replacement/restore demonstrated | Local restart: yes. Worker replacement: yes (crash → recovery → new worker, exercised against the real entrypoint). Restore: yes (local, with deletion registry). **Production profile: not run.** |
| Catalogue coverage and safe fallback for every family | Met |
| Pinned experimental dependencies with failure handling | Met (provider guard, ASP fencing, sandbox reconciliation) |
| Nothing declared complete because an interface, mock or diagram exists | Scripted/faux providers appear only behind explicit test-only flags (`MEMORY_TEST_SCRIPTED`, `MEMORY_TEST_SSH`); every live claim links to a report |

## 5. Not-qualified / not-run register

- Semantic qualification of any judgement family; all 31 remain `experimental` — no held-out gate
  is configured anywhere in this repository.
- Process substitution, long-history, cache/result-reuse, user-effort and scoped-execution studies.
- Deletion/recovery/historical-scope as a single continuous story (component-level only, §3.3).
- Production profile build/run, database failover drill, index-corruption drill.
- Persistence/rotation of runtime-issued worker credentials.
- Provider/image matrix beyond Docker and the bounded Daytona profile.
- Live formation quality: a recorded live run made incorrect retention decisions.
- Live campaign execution (`evals/campaign/`) — the current runner uses supplied fixture
  outcomes and costs. It does not launch workers; a real execution driver is still required.

## 6. What a user can rely on today

The local profile: authenticated API and CLI; durable jobs, fenced leases, budgets and receipts;
a worker pool that composes only against the services it actually has, claims work fairly,
survives crashes and provider outages, and never completes work with unexamined coverage; the
Host's own scheduler that carries formation, cohort consolidation, qualification and maintenance
forward without an operator submitting each job by hand; corrections that keep history and mark
dependent work; temporary explorations that stay out of retrieval; owner decisions that block
only their job; deletion that revokes before cleanup; backups that restore in isolation with the
deletion registry applied; and a full record of what each model was actually shown. What it
cannot yet promise is that the models' or any judgement provider's answers are *right* — every
retained record is a candidate and says so, and every judgement family is `experimental` until a
held-out gate says otherwise.
