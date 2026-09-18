# Release candidate — 18 September 2026

This is the WP16 release package for the memory architecture implementation. It freezes the
supported version matrix, states the active mode and fallback of every semantic judgement
family, and reports operational, semantic and behavioural results **separately**, as §21.4
and §22.17 require. It is a *release candidate*: the functional and operational evidence
below is complete for the local profile; semantic qualification is not, and says so.

## 1. Frozen version matrix

| Component | Version / revision | Evidence |
| --- | --- | --- |
| Rust toolchain | 1.96.0 (Cargo.lock pins crates) | `cargo fmt --check`, `cargo clippy -D warnings`, tests |
| Node / npm | 26.4.0 / 11.17.0 (`engines: >=22.19`) | `package-lock.json`; vitest 4.1.11 |
| SQLite | via `node:sqlite` (3.53.3 observed) and sqlx bundled driver | WP02/WP04 suites, backup `VACUUM INTO` |
| PostgreSQL | 14.17 (disposable cluster in `npm run test:store`) | store parity, Pi backend (57 tests), migration |
| Pi (harness, ai, chord, telemetry, sqlite backend) | `e4c75a73222ae2c72abb5f5314fa35ee8effc508`, packages 0.85.1, built from source | upstream conformance 54 in-memory + 105 SQLite |
| Harbor | `kobe0938/harbor` @ `dd4784b1ade1f446399e194f6dffd15142a77b98` (0.22.0) | `npm run test:docker` (Docker lifecycle), bounded Daytona profile |
| ASP | v0 SSH; reference `8ee7f0188b4c55aeca47d90b430da24bbbdbda89` | `npm run test:asp` (loopback OpenSSH) |
| Python | 3.14 (bridge, importer, store tests) | 28 bridge tests |
| Docker | 29.8 (local); Daytona bounded test profile | WP05 evidence in `plan.md` |
| Models used in live evidence | Azure OpenAI `gpt-4.1-mini` (judgement reference, formation/consolidation live runs), `gpt-5.4-mini` (task model in the live walkthrough), Jev `jev-latest` → `jev-1.13.0` (shadow) | `evals/` reports |
| Embeddings | Nomic local (`npm run embeddings`) | `evals/activation/nomic-smoke.json` |

Deployment profiles: **local** (single Host, SQLite, Host-spawned Node pool, Docker sandbox)
is implemented and exercised. **Production** (`deploy/`: PostgreSQL with separate roles,
file secrets, TLS proxy, Host + pool image) is written and syntax-validated; it has not been
built or run in this repository.

## 2. Judgement families: active mode and fallback

Every family J01–J29 is defined in `packages/judgement/src/catalogue-data.json` with its own
criteria, applicability, permitted uses and evaluation references. **Active mode for all 29
families: `shadow` — Jev's answer is recorded next to the reference model's answer; the
reference (Azure `gpt-4.1-mini`) decides.** No family is `qualified`.

Fallback for every family, as defined in the catalogue: *"Use the authorised
conventional-model route with the same evidence and questions; otherwise return unresolved
work for the owning process."* The runtime implements this: a failed or unparseable reply is
recorded as an `unavailable`/`invalid_response` assessment; the owning process defers the
item and reports it as unresolved rather than inventing an answer (exercised by the
provider-outage fault test).

| Family | Owner | Live evidence (shadow) | Status |
| --- | --- | --- | --- |
| J01, J02 | Formation | 20/20 replies per provider in `evals/formation/live` (Jev matched 20/20 expected retention decisions; Azure made 4 incorrect retentions); 21/21 packets agreeing in `evals/live-walkthrough` and the day's earlier runs; J01/J02 revision 2 in `evals/judgement/live-smoke-revised.json` | shadow, unqualified |
| J03, J04, J05 | Formation | scripted contract tests only | shadow, unqualified |
| J06–J10 | Activation / maintenance | scripted contract tests; Nomic retrieval smoke | shadow, unqualified |
| J11, J12 | Consolidation | one paid run in `evals/consolidation/live-2026-09-18.json` (judge accepted an unsupported generalisation) | shadow, unqualified |
| J13–J17 | Maintenance / intentions | scripted contract tests | shadow, unqualified |
| J18–J20 | Scoped harness / workspace | scripted contract tests | shadow, unqualified |
| J21, J22 | Results | scripted contract tests | shadow, unqualified |
| J23–J26 | Adaptive work | scripted contract tests; no runtime drives them yet (side-lane design agreed) | shadow, unqualified |
| J27, J28 | User interaction | scripted contract tests | shadow, unqualified |
| J29 | Scoped work | scripted contract tests | shadow, unqualified |

Qualification requires held-out positive, negative, missing-evidence, ambiguous and
conflicting cases per family and domain, compared against the conventional route, with a
threshold chosen from those results (`evals/judgement/README.md`). None exists yet.

## 3. Results, reported separately

### 3.1 Operational (deterministic fixtures and backend conformance)

Release gates: `npm run check` and `npm run test:store` — results recorded in §5. Coverage:
Rust contracts, store (SQLite and PostgreSQL), Host HTTP, authentication, runtime credentials,
pool claiming and fairness, child supervision, backup, operations endpoints; TypeScript Pi
worker, workspace, judgement contract, formation/activation/consolidation/maintenance
runtimes, CLI walkthrough, backup/restore, pool end-to-end with crash and outage faults;
Python bridge and ATIF importer; upstream Pi conformance. Fault injection exercised: worker
crash after claim, judgement provider outage, Host restart with pool restart, stale granted
input, expired lease recovery, lost commit receipt (WP04), host failure before/after commit.

### 3.2 Semantic (live model behaviour)

| Report | What it shows | What it does not show |
| --- | --- | --- |
| `evals/formation/live/README.md` | Formation pipeline runs live; Jev 20/20 expected retention decisions in shadow; Azure 4 incorrect retentions, 7 malformed replies before strict schemas | Qualification of any family |
| `evals/judgement/live-smoke*.json` | Batched and separate-family Jev calls agree; J01/J02 revision 2 fixed a criteria ambiguity | Held-out accuracy |
| `evals/consolidation/live-2026-09-18.json` | Live synthesis and advisory transfer run; judge accepted an unsupported generalisation | Consolidation retention quality |
| `evals/activation/nomic-smoke.json` | Two paraphrase cases retrieve correctly with local embeddings | Retrieval usefulness at scale |
| `evals/live-walkthrough/run-2026-09-18.json` | A model works a task from corrected memory; findings judged by Azure and Jev (agreement); records retained with provenance; pool-driven with a Host restart | Model or judge quality; n is tiny |

### 3.3 Behavioural (acceptance stories, §21)

| Story | Status |
| --- | --- |
| §21.1 Property investigation, reuse and revision (agent solves C, forms episode/claim/method, consolidates, reuses on D, intention completes; interruptions after settled response and lost receipt) | **Not run as one story.** Components exercised separately: interruption recovery (WP04 tests), formation/consolidation/intention fixtures, pool faults. The agent story is the first follow-on WP16 item; design agreed (side-lane J23/J24/J26 during the task, formation after). Blocked on the process-input contract for activation/consolidation/maintenance dispatch. |
| §21.2 Autonomous learning and human influence | **Partially exercised.** User side complete in the Part 1 walkthrough (scoped correction, teaching with counterexample, commitment, temporary branch, outstanding authority question while other work continues). Unattended routine formation/consolidation/upkeep "while the user is absent" is exercised only for formation (pool) — consolidation and maintenance are not pool-dispatched yet. |
| §21.3 Deletion, recovery and historical scope | **Exercised at component level.** WP13 tests cover packet/projection/summary/session copies, access blocked before purge, surviving supported claims, honest backup expiry; the restore test restores in isolation and keeps deleted content inaccessible via the registry; clean continuation tested. Not yet run as one continuous story. |
| §21.4 Complete-release criteria | See §4. |

## 4. Complete-release criteria (§21.4) — status

| Criterion | Status |
| --- | --- |
| Every applicable deterministic fixture and backend conformance test passes with no unexplained failures | Met for the local profile (§5); PostgreSQL parity met under a disposable cluster |
| A report for every semantic family and its active fallback | Met (§2): all 29 families shadow/unqualified with the catalogue fallback; live evidence where it exists |
| Predeclared quality/resource requirements | Resource bounds met (budgets, reservations, payload limits); **semantic quality targets not declared or met** |
| Local restart and production worker replacement/restore demonstrated | Local restart: yes (CLI walkthrough test, live run with pool restart). Worker replacement: yes (crash → recovery → new worker). Restore: yes (local, with deletion registry). **Production profile: not run.** |
| Every §23 requirement has an owner, implementation location, test and artifact | `docs/implementation/traceability.md` resolves WP01–WP15 to code, tests and evidence; rows marked unqualified/open are listed in §6 |
| Catalogue coverage and safe fallback for every Jev family | Met |
| Pinned experimental dependencies with failure handling | Met (`compatibility.md`; provider guard, ASP fencing, sandbox reconciliation) |
| Nothing declared complete because an interface, mock or diagram exists | Scripted providers appear only in tests; every live claim links to a report |

## 5. Release gates

Run on 18 September 2026 from the working folder, after all WP15 changes:

| Gate | Result |
| --- | --- |
| `npm run check` (fmt, Clippy, generate, build, Rust tests, vitest, bridge, upstream) | **passed** — 38 Rust tests (11 PostgreSQL-gated ignored), 148 TypeScript tests (71 opt-in/backend skipped), 28 bridge tests (7 skipped), 54 upstream in-memory + 105 upstream SQLite |
| `npm run test:store` (both backends under a disposable PostgreSQL 14.17 cluster) | **passed** — 22 Rust tests including interaction parity and SQLite→PostgreSQL migration, 57 PostgreSQL Pi backend tests |
| Paid opt-in live walkthrough (`MEMORY_LIVE_WALKTHROUGH=1`) | passed earlier the same day; transcript `evals/live-walkthrough/run-2026-09-18.json` |
| `docker-compose -f deploy/compose.yaml config` | valid (syntax only) |

No failures were skipped or explained away; every ignored or skipped test is an opt-in
backend or paid-provider case whose gating is stated in its file.

## 6. Not-qualified / not-run register

- Semantic qualification of any judgement family; all remain shadow.
- Process substitution, long-history, cache/result-reuse, user-effort and scoped-execution studies.
- §21.1 as a single story; §21.2 unattended consolidation/maintenance; §21.3 as a single story.
- Pool dispatch for activation, consolidation, maintenance (process-input contract).
- Production profile build/run, database failover drill, index-corruption drill.
- Persistence/rotation of runtime-issued worker credentials.
- Provider/image matrix beyond Docker and the bounded Daytona profile; Azure `gpt-5.4-mini`
  used for the task model in one day's runs only.
- Live formation quality: Azure reference made four incorrect retentions in the recorded run.

## 7. What a user can rely on today

The local profile: authenticated API and CLI; durable jobs, fenced leases, budgets and
receipts; a worker pool that claims fairly, survives crashes and provider outages, and never
completes work with unexamined coverage; corrections that keep history and mark dependent
work; temporary explorations that stay out of retrieval; owner decisions that block only
their job; deletion that revokes before cleanup; backups that restore in isolation with the
deletion registry applied; and a full record of what each model was actually shown. What it
cannot yet promise is that the models' or Jev's judgements are *right* — every retained
record is a candidate and says so.
