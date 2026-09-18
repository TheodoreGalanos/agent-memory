# Agent Memory — conceptual-to-implementation traceability

**Version 1.0 · 17 September 2026**

[conceptual-specification.md](conceptual-specification.md) is the source baseline. Every numbered section and substantive subsection is mapped below; line ranges refer to that exact attached Markdown. A mapping indicates planned ownership and verification, not completed implementation. Navigation/title material is excluded. Rationale and references are mapped as source provenance.

**Coverage:** 104 source sections/subsections; all mapped. The implementation has 16 work packages, 15 test suites and 29 judgement families.

| Requirement ID / source | Implementation | Work packages / verification |
| --- | --- | --- |
| **C-1-043** — 1. System composition<br>Source L43–L69 | 1–2, 4–9 | WP01–WP07<br>T01–T06 |
| **C-1-053** — How the components cooperate<br>Source L53–L60 | 1–2, 4–9 | WP01–WP07<br>T01–T06 |
| **C-1-061** — Scoped execution<br>Source L61–L69 | 1–2, 4–9 | WP01–WP07<br>T01–T06 |
| **C-2-070** — 2. Memory functions<br>Source L70–L105 | 3, 8, 11–15 | WP02, WP06, WP09–WP12<br>T01, T05, T07–T10 |
| **C-2-083** — Two forms of procedural memory<br>Source L83–L90 | 3.3, 13.2–13.3 | WP02, WP11<br>T01, T09 |
| **C-2-091** — User-scoped preferences<br>Source L91–L96 | 3.1, 11.2, 18 | WP02, WP09, WP14<br>T07, T13 |
| **C-2-097** — One experience, several useful representations<br>Source L97–L105 | 3, 8, 11–15 | WP02, WP06, WP09–WP12<br>T01, T05, T07–T10 |
| **C-3-106** — 3. Workspace, budget and rendering<br>Source L106–L174 | 6, 8–9, 17–18 | WP04, WP06–WP07, WP13–WP14<br>T03, T05–T06, T12–T13 |
| **C-3-110** — Workspace lifetime<br>Source L110–L115 | 8.1, 17.2 | WP06, WP13<br>T05, T12 |
| **C-3-116** — Exploration and selective transfer<br>Source L116–L123 | 8.1, 18.2 | WP06, WP14<br>T05, T13 |
| **C-3-124** — Working material and rendered context<br>Source L124–L131 | 6, 8–9, 17–18 | WP04, WP06–WP07, WP13–WP14<br>T03, T05–T06, T12–T13 |
| **C-3-132** — From candidates to a bounded context<br>Source L132–L141 | 6, 8–9, 17–18 | WP04, WP06–WP07, WP13–WP14<br>T03, T05–T06, T12–T13 |
| **C-3-142** — Cache-aware rendering<br>Source L142–L149 | 8.2–8.3, 19.5 | WP06, WP16<br>T05, T15 |
| **C-3-150** — Origin and lineage in the workspace<br>Source L150–L165 | 6, 8–9, 17–18 | WP04, WP06–WP07, WP13–WP14<br>T03, T05–T06, T12–T13 |
| **C-3-166** — Preserving unresolved disagreements<br>Source L166–L169 | 6, 8–9, 17–18 | WP04, WP06–WP07, WP13–WP14<br>T03, T05–T06, T12–T13 |
| **C-3-170** — Continuity across context changes<br>Source L170–L174 | 6, 8–9, 17–18 | WP04, WP06–WP07, WP13–WP14<br>T03, T05–T06, T12–T13 |
| **C-4-175** — 4. Connected memory and time<br>Source L175–L218 | 3, 12, 14 | WP02, WP10, WP12<br>T01, T08, T10 |
| **C-4-193** — Inspectable records and evidence<br>Source L193–L198 | 3, 12, 14 | WP02, WP10, WP12<br>T01, T08, T10 |
| **C-4-199** — Views over connected memory<br>Source L199–L204 | 3, 12, 14 | WP02, WP10, WP12<br>T01, T08, T10 |
| **C-4-205** — Bitemporal history<br>Source L205–L218 | 3.5, 14.1 | WP02, WP12<br>T01, T10 |
| **C-5-219** — 5. Four peer processes<br>Source L219–L299 | 9–16 | WP07–WP12<br>T06–T11 |
| **C-5-223** — Formation — preserve useful experience<br>Source L223–L229 | 11 | WP09<br>T07 |
| **C-5-230** — Activation — allocate present attention<br>Source L230–L238 | 12 | WP10<br>T08 |
| **C-5-239** — Consolidation — develop abstractions and methods<br>Source L239–L247 | 13 | WP11<br>T09 |
| **C-5-248** — Maintenance — keep retained memory current<br>Source L248–L256 | 14–15 | WP12<br>T10 |
| **C-5-257** — Learning from user contributions<br>Source L257–L264 | 13.1–13.4, 18.2 | WP11, WP14<br>T09, T13 |
| **C-5-265** — Execution across the four processes<br>Source L265–L275 | 9–16 | WP07–WP12<br>T06–T11 |
| **C-5-276** — Semantic assessments<br>Source L276–L279 | 9–16 | WP07–WP12<br>T06–T11 |
| **C-5-280** — The scoped operation<br>Source L280–L291 | 9, Appendix A.1 | WP07<br>T06 |
| **C-5-292** — Results and integration<br>Source L292–L299 | 5.4–5.5, 9.1–9.4 | WP03, WP07<br>T02, T06 |
| **C-6-300** — 6. Shared policy and observable use<br>Source L300–L379 | 4–5, 10, 16–19 | WP03, WP08, WP13–WP14<br>T02, T11–T13, T15 |
| **C-6-302** — Policy has an owner<br>Source L302–L309 | 4–5, 10, 16–19 | WP03, WP08, WP13–WP14<br>T02, T11–T13, T15 |
| **C-6-310** — Autonomous operation and user direction<br>Source L310–L315 | 10.1, 18 | WP03, WP14<br>T13 |
| **C-6-316** — Communication and requests for user input<br>Source L316–L323 | 4–5, 10, 16–19 | WP03, WP08, WP13–WP14<br>T02, T11–T13, T15 |
| **C-6-324** — Starting policy<br>Source L324–L334 | 4–5, 10, 16–19 | WP03, WP08, WP13–WP14<br>T02, T11–T13, T15 |
| **C-6-335** — Policy for bounded judgements<br>Source L335–L340 | 4–5, 10, 16–19 | WP03, WP08, WP13–WP14<br>T02, T11–T13, T15 |
| **C-6-341** — Operation budgets<br>Source L341–L348 | 5.6, 9.3 | WP03, WP07<br>T02, T06 |
| **C-6-349** — Reuse of completed work<br>Source L349–L354 | 9.4, 16.6, 19.5 | WP07, WP08, WP16<br>T06, T11, T15 |
| **C-6-355** — Available, selected and applied<br>Source L355–L379 | 8.2–8.4, 16.6, 19.6 | WP06, WP08<br>T05, T11 |
| **C-7-380** — 7. Three interacting loops<br>Source L380–L408 | 5, 9, 11–15, 21 | WP03, WP07, WP09–WP12, WP16<br>T02, T06–T10, T15 |
| **C-7-388** — Action<br>Source L388–L391 | 5, 9, 11–15, 21 | WP03, WP07, WP09–WP12, WP16<br>T02, T06–T10, T15 |
| **C-7-392** — Learning<br>Source L392–L395 | 5, 9, 11–15, 21 | WP03, WP07, WP09–WP12, WP16<br>T02, T06–T10, T15 |
| **C-7-396** — Intention<br>Source L396–L399 | 5, 9, 11–15, 21 | WP03, WP07, WP09–WP12, WP16<br>T02, T06–T10, T15 |
| **C-7-400** — Context and timing<br>Source L400–L408 | 5, 9, 11–15, 21 | WP03, WP07, WP09–WP12, WP16<br>T02, T06–T10, T15 |
| **C-8-409** — 8. Intention lifecycle and ownership<br>Source L409–L448 | 5, 15, 18 | WP03, WP12, WP14<br>T02, T10, T13 |
| **C-8-428** — User-visible commitments<br>Source L428–L435 | 5, 15, 18 | WP03, WP12, WP14<br>T02, T10, T13 |
| **C-8-436** — Triggers, results and completion<br>Source L436–L441 | 5, 15, 18 | WP03, WP12, WP14<br>T02, T10, T13 |
| **C-8-442** — Expiry and terminal outcomes<br>Source L442–L448 | 5, 15, 18 | WP03, WP12, WP14<br>T02, T10, T13 |
| **C-9-449** — 9. Revision, support and active work<br>Source L449–L508 | 3, 5, 8, 14, 17 | WP02–WP03, WP06, WP12–WP13<br>T01–T02, T05, T10, T12 |
| **C-9-451** — Three independent assessments<br>Source L451–L458 | 3, 5, 8, 14, 17 | WP02–WP03, WP06, WP12–WP13<br>T01–T02, T05, T10, T12 |
| **C-9-459** — Revision reaches the workspace as an event<br>Source L459–L466 | 3, 5, 8, 14, 17 | WP02–WP03, WP06, WP12–WP13<br>T01–T02, T05, T10, T12 |
| **C-9-467** — Task-local dependencies<br>Source L467–L472 | 8.4, 14.2 | WP06, WP12<br>T05, T10 |
| **C-9-473** — User-supplied corrections<br>Source L473–L480 | 14.1–14.2, 18.2 | WP12, WP14<br>T10, T13 |
| **C-9-481** — Results from scoped work<br>Source L481–L484 | 3, 5, 8, 14, 17 | WP02–WP03, WP06, WP12–WP13<br>T01–T02, T05, T10, T12 |
| **C-9-485** — Disagreement and historical succession<br>Source L485–L496 | 3, 5, 8, 14, 17 | WP02–WP03, WP06, WP12–WP13<br>T01–T02, T05, T10, T12 |
| **C-9-497** — Deletion and support reassessment<br>Source L497–L508 | 14.4, 17 | WP12–WP13<br>T10, T12 |
| **C-10-509** — 10. Worked example: a source revision<br>Source L509–L533 | 11–16, 21.1 | WP09–WP12, WP16<br>T07–T11, T15 |
| **C-10-515** — Scoped investigation<br>Source L515–L524 | 11–16, 21.1 | WP09–WP12, WP16<br>T07–T11, T15 |
| **C-10-525** — What the sequence establishes<br>Source L525–L533 | 11–16, 21.1 | WP09–WP12, WP16<br>T07–T11, T15 |
| **C-11-534** — 11. System behaviours and review criteria<br>Source L534–L556 | 19, 21, 23 | WP01–WP16<br>T01–T15 |
| **C-11-549** — Review operations and outcomes together<br>Source L549–L556 | 19, 21, 23 | WP01–WP16<br>T01–T15 |
| **C-12-557** — 12. Process testing and evaluation<br>Source L557–L715 | 19, 21–23 | WP01–WP16<br>T01–T15 |
| **C-12-559** — Test structure and levels<br>Source L559–L572 | 19, 21–23 | WP01–WP16<br>T01–T15 |
| **C-12-573** — Formation — capture and selection<br>Source L573–L582 | 11.3, 19 | WP09<br>T07 |
| **C-12-583** — Activation — relevance and applicability<br>Source L583–L592 | 12.4, 19 | WP10<br>T08 |
| **C-12-593** — Consolidation — abstraction and transfer<br>Source L593–L602 | 13.3–13.5, 19 | WP11<br>T09 |
| **C-12-603** — Maintenance — change and preservation<br>Source L603–L612 | 14.5, 15.4, 19 | WP12<br>T10 |
| **C-12-613** — Workspace and scoped-execution tests<br>Source L613–L626 | 19.2 | WP04–WP07<br>T03–T06 |
| **C-12-627** — Cache and result reuse<br>Source L627–L632 | 8.3, 9.4, 19.5 | WP06–WP07, WP16<br>T05–T06, T15 |
| **C-12-633** — Evaluator checks<br>Source L633–L644 | 19.3 | WP01, WP16<br>Evaluator regression within T15 |
| **C-12-645** — Scenario library<br>Source L645–L664 | 19, 21–23 | WP01–WP16<br>T01–T15 |
| **C-12-665** — Scoped execution scenarios<br>Source L665–L674 | 19, 21–23 | WP01–WP16<br>T01–T15 |
| **C-12-675** — Interaction scenarios<br>Source L675–L688 | 18, 19.2, 21.2 | WP14, WP16<br>T13, T15 |
| **C-12-689** — From isolated tests to system value<br>Source L689–L696 | 19.4–19.5, 21 | WP16<br>T15 |
| **C-12-697** — Semantic judgement integration<br>Source L697–L702 | 16, 19.2–19.4 | WP08, WP16<br>T11, T15 |
| **C-12-703** — Evidence captured for review<br>Source L703–L715 | 19, 21–23 | WP01–WP16<br>T01–T15 |
| **C-13-716** — 13. Design rationale<br>Source L716–L743 | 2, Appendix B | WP01<br>Source/compatibility review |
| **C-13-720** — Cognitive and biological foundations<br>Source L720–L729 | 2, Appendix B | WP01<br>Source/compatibility review |
| **C-13-730** — Published architecture patterns<br>Source L730–L743 | 2, Appendix B | WP01<br>Source/compatibility review |
| **C-14-744** — 14. References<br>Source L744–L775 | Appendix B | WP01<br>Source provenance |
| **C-A-776** — Appendix A. Structured semantic judgement<br>Source L776–L966 | 16, 19, Appendix B | WP08–WP14, WP16<br>T07–T13, T15 |
| **C-A-780** — A.1 Placement and ownership<br>Source L780–L791 | 1–2, 10, 16.1 | WP01, WP08<br>T01, T11 |
| **C-A-792** — A.2 Uses across memory and work<br>Source L792–L825 | 16.3, 11–15, 18 | WP08–WP14<br>J01–J29; T07–T13 |
| **C-A-796** — Formation<br>Source L796–L801 | 16, 19, Appendix B | WP08–WP14, WP16<br>T07–T13, T15 |
| **C-A-802** — Activation<br>Source L802–L807 | 16, 19, Appendix B | WP08–WP14, WP16<br>T07–T13, T15 |
| **C-A-808** — Consolidation<br>Source L808–L813 | 16, 19, Appendix B | WP08–WP14, WP16<br>T07–T13, T15 |
| **C-A-814** — Maintenance<br>Source L814–L819 | 16, 19, Appendix B | WP08–WP14, WP16<br>T07–T13, T15 |
| **C-A-820** — Shared work and user contributions<br>Source L820–L825 | 16, 19, Appendix B | WP08–WP14, WP16<br>T07–T13, T15 |
| **C-A-826** — A.3 Preparing the question and evidence<br>Source L826–L851 | 16.1–16.2, Appendix A.1 | WP08<br>T11 packet/contract |
| **C-A-840** — Choosing an answer form<br>Source L840–L851 | 16.1–16.2 | WP08<br>T11 response semantics |
| **C-A-852** — A.4 From an assessment to further work<br>Source L852–L871 | 10, 16.2–16.4 | WP08<br>T11 routing |
| **C-A-860** — Combining judgement and investigation<br>Source L860–L865 | 16, 19, Appendix B | WP08–WP14, WP16<br>T07–T13, T15 |
| **C-A-866** — Interpreting confidence and failure<br>Source L866–L871 | 16.2, 16.4 | WP08, WP16<br>T11 calibration/fallback |
| **C-A-872** — A.5 Reusable checks, provenance and cost<br>Source L872–L897 | 5.6, 13.4, 16.5–16.6 | WP08, WP11<br>T02, T09, T11 |
| **C-A-874** — From a temporary question to an established check<br>Source L874–L879 | 16, 19, Appendix B | WP08–WP14, WP16<br>T07–T13, T15 |
| **C-A-880** — Keep the assessment distinct from the decision<br>Source L880–L887 | 16, 19, Appendix B | WP08–WP14, WP16<br>T07–T13, T15 |
| **C-A-888** — Spend and reuse within the operation's allowance<br>Source L888–L897 | 16, 19, Appendix B | WP08–WP14, WP16<br>T07–T13, T15 |
| **C-A-898** — A.6 Worked example: a property across revisions<br>Source L898–L917 | 21.1 | WP16<br>T15 |
| **C-A-902** — Retain the supported finding<br>Source L902–L905 | 16, 19, Appendix B | WP08–WP14, WP16<br>T07–T13, T15 |
| **C-A-906** — Prepare and carry out the D investigation<br>Source L906–L911 | 16, 19, Appendix B | WP08–WP14, WP16<br>T07–T13, T15 |
| **C-A-912** — Learn from the cases and reconcile the change<br>Source L912–L917 | 16, 19, Appendix B | WP08–WP14, WP16<br>T07–T13, T15 |
| **C-A-918** — A.7 Evaluating the integration<br>Source L918–L934 | 19.1–19.4 | WP08, WP16<br>T11, T15 |
| **C-A-935** — A.8 Adoption and review criteria<br>Source L935–L966 | 16.3–16.4, 21.4 | WP08, WP16<br>J01–J29 qualification |
| **C-A-950** — Source basis<br>Source L950–L966 | Appendix B | WP01<br>Source provenance |

## Using this matrix

For each row, the implementation team adds code/test locations and the release evidence identifier. A row is complete when the described runtime behaviour and its failure paths are implemented and assessed at the appropriate test level. Source rationale does not add an unrequested feature. The main specification §23 provides the compact grouped view.


## WP01 implementation locations

These locations provide the first contract and compatibility evidence. They do not
mark the broader behavioural requirements or complete T01/T03/T15 suites finished.

| Source / implementation requirement | Code and verification |
| --- | --- |
| C-1-043, C-1-061; implementation §§2, 4 and Appendix A | `crates/memory-domain/src/contracts.rs`; generated files in `contracts/generated/`; Rust `tests/contracts.rs` and TypeScript `test/contracts.test.ts`. |
| C-4-175, C-9-451; scoped references and evidence status | `MemoryRef`, `SourceRef`, `Scope`, `Finding`; scope narrowing, revision bounds and finding/input tests. Temporal persistence belongs to WP02. |
| C-5-280, C-5-292; scoped briefs/results | `Command<WorkBrief>` and `WorkResult`; nested scope, deadline, partial coverage and unknown-effect checks. |
| C-12-559, C-12-613; harness/fixture infrastructure | `packages/pi-worker/src/harness.ts`, `packages/pi-worker/test/harness.test.ts`; upstream in-memory and SQLite conformance scripts. |
| C-12-633, C-A-898; evaluator separation and C/D checkpoints | `evals/property-location/inputs/`, `results/`, `reference-world.json`; evidence-cutoff and non-disclosure assertions. Scripted responses only; later packages assess process quality. |
| C-13-716, C-A-950; source compatibility | `contracts/compatibility.json`, `contracts/upstream/`, `docs/implementation/compatibility.md`; actual compiled APIs and documented upstream gaps. |

Paths are relative to the project root, except `tests/contracts.rs`, which is inside
`crates/memory-domain`, and `test/contracts.test.ts`, which is inside `packages/pi-worker`.


## WP02 implementation locations

The shared behavior below is verified on SQLite and PostgreSQL; see [storage](storage.md).

| Requirement | Code and verification |
| --- | --- |
| C-2-070, C-2-083; implementation §§3.1–3.3 | `crates/memory-domain/src/records.rs`; `memory-store/src/memories.rs`; all four families, independent origin/evidence/lifecycle state and atomic mixed batches. |
| C-4-175, C-4-205, C-9-485; §§3.4–3.5 | `memory-store/src/relations.rs`, `temporal.rs`; direction, conflict deduplication, derivation cycles, common ancestry, historical corrections and late world changes. |
| C-2-091, C-4-193; §3.1 | `memory-store/src/access.rs`, `entities.rs`; tenant/user/project/task/entity/source grants, explicit identity links and mutation restrictions. |
| C-9-451, C-A-898; §3.6 | `memory-store/src/sources/`; revision snapshots, unavailable evidence, source locators, CSV/model/tool-event adapters and the shared C/D property-location fixture. |
| C-9-497; artifact parts of T12/T14 | `memory-store/src/artifacts.rs`; interrupted upload/recovery, late completion, bounded reads/export and dependency revocation. Physical purge remains WP13. |
| T01, artifact parts of T12/T14 | `crates/memory-store/tests/repositories.rs`; same suite called for SQLite/PostgreSQL, plus migration upgrades. `scripts/test-store.py` owns the disposable PostgreSQL cluster. |

`memory-store/...` paths in this table are beneath `crates/`. Rust repository types
remain internal to the host; the WP03 API uses generated transport types.

## WP03–WP05 implementation locations

| Requirement | Code and verification |
| --- | --- |
| T02, early T12/T14; implementation §5 | `crates/memory-store/src/coordinator/` and `tests/coordinator.rs`: job/event atomicity, receipts, claims, waits, cancellation, budgets, effects and recovery on SQLite/PostgreSQL. |
| T02; authenticated assignments | `crates/memory-host/src/`; `tests/authentication.rs`; `packages/pi-worker/test/http.test.ts`: scoped roles, expiry, assignment binding and actual HTTP commands. |
| T02/T03; result acknowledgement | `memory-host/tests/result_publication.rs`, `pi-worker/test/host-worker.test.ts`, `assigned-worker.test.ts`: real host/worker publication plus lost-response, retry and deferred recovery. |
| T03/T14; Pi ownership and persistence | `packages/pi-worker/src/local-session.ts`, `postgres-session.ts`; corresponding tests: kernel lock/process death, backup, transactional ownership and upstream backend conformance. |
| T04; remote execution | `packages/pi-worker/src/asp-execution-env.ts`, `services/harbor-bridge/asp_helper.py`; `test/asp.test.ts`, `test_helper.py`: binary/path behavior, bounded output and process-group cancellation via real helper/SSH. |
| T04/T14; provider lifecycle | `services/harbor-bridge/lifecycle.py`, `docker_provider.py`, `daytona_provider.py`, `image/`; `test_lifecycle.py`, `test_daytona.py`, `memory-host/tests/sandbox_lifecycle.rs`, `pi-worker/test/sandbox.test.ts`: allocation receipts, protected endpoint, restart/orphan reconciliation, export publication and verified deletion. Docker and the bounded Daytona profile passed; limits are recorded in the plan. |

Paths beginning `memory-host/` are beneath `crates/`; `pi-worker/` is beneath
`packages/`. These tests cover the implemented increments, not all T01–T15 release
requirements. The [plan](plan.md) records WP05 evidence and the remaining work packages.


## WP06 implementation locations

| Requirement | Code and verification |
| --- | --- |
| Implementation §8.1; T05 | `crates/memory-domain/src/workspace.rs`, generated workspace/manifest contracts; `packages/pi-worker/src/workspace.ts`: typed working state, origin, evidence exposure, independent scratch/recovery lifetime and temporary contribution selection. |
| §§8.2–8.3; T05 | `packages/pi-worker/src/workspace-harness.ts`: complete conflict groups, bounded context, source coverage, persisted actual selection, explicit deferrals and rebuild reasons. `test/workspace.test.ts` checks payload expansion, cache metric availability and access changes. |
| §8.4; T05 | `workspace.ts`: task-local substantive-change reconciliation; tests preserve completed historical scope and ignore wording-only or unrelated valid-time changes. |
| §§6.3–6.5; T03/T05 | `harness.ts`, `provider-guard.ts`, `assigned-worker.ts`: custom projectors, structural compaction, destination-branch recovery, live assignment checks and provider-call rejection after hook failures. |
| T03/T05; local/production persistence | `test/workspace.test.ts` and `test/postgres.test.ts`: real Pi compaction/checkpoint/manifest restoration on SQLite and PostgreSQL. |

Live model qualification, physical erasure and maintenance scheduling remain
assigned to their later packages. Formation writes are covered by WP09 below. See the runtime guide for the
current integration contract and the plan for executed checks.


## WP07 implementation locations

| Requirement | Code and verification |
| --- | --- |
| §9.1–9.2; T06 | `crates/memory-domain/src/scoped.rs`, `packages/pi-worker/src/scoped-operation.ts`: typed questions, evidence and output criteria; bounded JSON inspection and definition expansion. |
| §9.3; T02/T03/T06 | `memory-store/src/coordinator/scoped.rs`, `jobs.rs`, migration 004 and `tests/coordinator.rs`: independent child jobs, narrowing permissions, root limits, dependency waits, cancellation and recovery on both databases. |
| §9.4; T06 | `coordinator/scoped.rs`: explicit completed-result reuse, current access checks and rejection after artifact revocation, including duplicate spawn receipts. |
| §9.2; T05/T06 | `pi-worker/src/assigned-worker.ts`, `scoped-operation.ts`; `test/investigation-worker.test.ts`, `test/scoped-operation.test.ts` and `memory-host/tests/result_publication.rs`: real HTTP/Pi wait/reopen, expanded definition, child counterexample, missing coverage and bounded partial output. |
| §7.5, §9.3; T04/T06 | `pi-worker/src/interpreter-client.ts`, `interpreter-tool.ts`, `services/harbor-bridge/interpreter.py`: persistent sandbox objects, selected JSON checkpoints, effect reconciliation, wall limits and explicit heap restoration. Tests: `interpreter-client.test.ts`, `test_interpreter.py`, real SSH `asp.test.ts` and Docker `sandbox.test.ts`. |

These checks use scripted model responses. They establish execution and recovery
behavior, not live model judgement quality. The updated Daytona image requires
separate paid qualification; the current WP07 evidence uses the local Docker profile.

## WP08 implementation locations

- **T11 definitions and packets:** `packages/judgement/src/catalogue-data.json`,
  `packet.ts`, `validation.ts`; `crates/memory-domain/src/judgement.rs` and its
  generated contracts. All J01–J29 have authored packet/answer/disposition/fallback
  fixtures in `evals/judgement/fixtures.json` and parameterized contract tests.
- **T11 execution:** `packages/judgement/src/providers.ts`, `runtime.ts` and
  `policy.ts`; real Jev HTTP and Pi conventional-model routes, selected-question
  batching, retries, qualification gates and separate decisions.
- **T11/T12 host authority and lineage:** `crates/memory-host/src/judgements.rs`,
  `crates/memory-store/src/judgements.rs` and migration 005. The host verifies
  evidence contents, access and dependencies. Reuse checks current versions,
  scope, freshness and release identity. Task-local checks remain investigative.
- **Integration evidence:** `crates/memory-host/tests/judgement.rs` invokes
  `packages/judgement/test/host-runtime.test.ts` through real HTTP and Pi SQLite
  sessions. `scripts/test-store.py` repeats the host integration on PostgreSQL.
  Provider transport tests cover timeout, cancellation, malformed and oversized
  replies; contract tests distinguish invalid output from missing evidence.
- **Qualification limit:** the live smoke report records batching agreement and
  semantic disagreements. No family is enabled for real Jev automation. Domain
  accuracy, threshold calibration and later process effects remain separate from
  WP08 contract completion.

## WP09 implementation locations

| Requirement | Code and verification |
| --- | --- |
| §§10.3, 11.1; T07 | `memory-domain/src/formation.rs`, `memory-host/src/formation.rs`: typed capture, episode/cutoff bounds, explicit selection and host-owned candidate validation. |
| §§11.1–11.2; T07/T11 | `packages/formation/src/index.ts`: Pi recovery and selected J01/J02/J03/J04/J05/J27 packets using WP08 providers. |
| §§11.1–11.3; T01/T07 | `memory-store/src/formation.rs`, migration 006: atomic records, derivations, correction links, event receipts and cursor; common-source and deferred-event preservation. |
| §10.3; T07 | `services/harbor-bridge/trajectory_import.py`, `test_trajectory_import.py`: installed Harbor ATIF validation, observed/declarative distinctions, simulation, missing telemetry and uninspected media references. |
| §11.3; T07 | `evals/formation/capture.json`, `memory-host/tests/formation.rs`, `packages/formation/test/host-runtime.test.ts`: pre/post correction meaning, four candidate families, user scope, scratch exclusion, selected simulation, native links, duplicates, distinct same-text events, lost response recovery, evidence substitution and batch rollback. `scripts/test-store.py` runs the HTTP/Pi integration on PostgreSQL as well as SQLite. |

Live semantic qualification and held-out continuation benefits are not established
by the scripted formation tests. Intention transitions and maintenance remain WP12.

## WP10 implementation locations

| Requirement | Code and verification |
| --- | --- |
| §§12.1–12.3; T08 | `memory-domain/src/activation.rs`, `memory-store/src/activation.rs`, migration 007: scoped entity, lexical (FTS5/`tsvector`) and exact-vector retrieval, current-version checks, conflict groups and grouped context delivery. |
| §12.2; T08/T11 | `packages/activation/src/index.ts`: `runActivation`, J06–J10 packets through WP08 providers, `applyActivation` into the WP06 workspace; `embeddings.ts` with local Nomic indexing (`npm run embeddings`). |
| §12.3; T08 | `memory-host/src/activation.rs`, `memory-host/tests/activation.rs`, `memory-store/tests/activation.rs`, `packages/activation/test/*`: Host/Pi flows on SQLite and PostgreSQL; `evals/activation/nomic-smoke.json`, `next-step-probes.json`. Broader retrieval usefulness and a paid embedding/ANN profile remain unqualified. |

## WP11 implementation locations

| Requirement | Code and verification |
| --- | --- |
| §§13.1–13.4; T09 | `memory-domain/src/consolidation.rs`, `memory-store/src/consolidation.rs`, `memory-host/src/consolidation.rs`, `qualification.rs`, migration 008: source groups, conditional proposals, procedure manifests, child evaluation jobs and policy-governed adoption. |
| §13.2; T09/T11 | `packages/consolidation/src/index.ts`: `runConsolidation` with J11/J12 packets and an application-supplied synthesis function. |
| §13.4; T09 | `memory-host/tests/consolidation.rs`, `packages/consolidation/test/*`, `evals/consolidation/transfer-smoke.json`, `live-2026-09-18.json` (paid Azure synthesis; Jev shadow J11/J12; judge accepted an unsupported generalisation — retention unqualified). |

## WP12 implementation locations

| Requirement | Code and verification |
| --- | --- |
| §§14.1–14.2, 15; T10 | `memory-domain/src/{maintenance,intentions}.rs`, `memory-store/src/{maintenance,intentions}.rs`, `memory-host/src/{maintenance,intentions}.rs`, migration 009: revisions with preserved temporal meaning, source-loss retirement, change notices, intention occurrences, triggers, checked completion, cancellation, expiry, recurrence. |
| §14.2; T10 | `packages/maintenance/src/{index,workspace}.ts`: `runMaintenance` (J13–J16), `runIntentionCheck` (J17), `reconcileActiveWork` marking dependent workspace conclusions for recheck. |
| §15; T10 | `memory-store/tests/maintenance.rs`, `memory-host/tests/maintenance.rs`, `packages/maintenance/test/*`: all trigger routes, retry on one occurrence, capability escalation rejection, current-lease enforcement; both database backends. |

## WP13 implementation locations

| Requirement | Code and verification |
| --- | --- |
| §§17.3–17.5; T12 | `memory-domain/src/retention.rs`, `memory-store/src/retention.rs`, `memory-host/src/lib.rs` (deletion commands), migration 010: revocation epochs before cleanup, exposure registry, cross-store purge planner, support-versus-derivation handling, acknowledgements, restore-time deletion registry. |
| §17.4; T12/T03 | `packages/pi-worker/src/local-session.ts` (`purgeLocalSession`, `applyLocalDeletionRegistry`), `postgres-session.ts` (deletion registry, fenced writers), clean continuation in `assigned-worker.ts`. |
| §17; T12/T14 | `memory-store/tests/retention.rs`, `memory-host/tests/retention.rs`, `packages/pi-worker/test/{local-session,postgres}.test.ts`; `restore_registry` in `memory-host/src/main.rs` applied before serving. |

## WP14 implementation locations

| Requirement | Code and verification |
| --- | --- |
| §18 (all eight experiences), §10.3 connectors; T13 | `memory-domain/src/interaction.rs`, `memory-store/src/interaction.rs`, `interaction/mutations.rs`, migration 011: scoped reads, retry-safe mutations, corrections with change notices, temporary explorations, prepared owner decisions, notification preferences, dispatch controls; `HostRequest::IngestSource` for trusted connectors. |
| §18; T13 | `packages/cli/src/main.ts` (`init`, `enable-pool`, named reads, `mutate`, `call`, `backup`, `restore`), `examples/walkthrough.mjs`, `examples/live-walkthrough.ts`, `docs/user-guide.md`. |
| §18, §16 shadow use; T13 | `memory-store/tests/interaction.rs` (SQLite and PostgreSQL), `memory-host/tests/sources.rs`, `packages/cli/test/walkthrough.test.ts`, `examples/test/*`; paid opt-in `examples/test/live-walkthrough.test.ts` with transcript `evals/live-walkthrough/run-2026-09-18.json` (Azure reference, Jev shadow; agreement on all packets; not qualification). |

## WP15 implementation locations

| Requirement | Code and verification |
| --- | --- |
| §20.1, §17.1 credentials; T14 | `memory-host/src/lib.rs` (`Role::Pool`, `IssueWorkerCredential`, `ClaimNext` with fair ordering, `Backup`, operations view), `memory-host/src/supervisor.rs` (Host-spawned pool children with backoff), `memory-host/src/main.rs` (recovery timer, `worker_pool`), `memory-host/src/http.rs` (`/healthz`, `/readyz`, `/metrics`), `memory-store/src/coordinator/jobs.rs` (`queued_jobs`). |
| §20.1–20.2 worker pools | `packages/supervisor/src/{index,task,trace,main}.ts`: class-separated slots, investigation and formation dispatch, strict `WorkResult` output, stale input → blocked, lease release, judgement trace. |
| §20.3–20.4 | `memory-store/src/database.rs` (`snapshot_sqlite`, `backup_position`), `memory-store/src/migrate.rs`, `src/bin/memory-migrate.rs`, CLI `backup`/`restore`; `deploy/` manifests (syntax-validated only). |
| §20.5 runbooks; T14 | `docs/runbooks.md` (13 entries, exercised/documented marked); tests `memory-host/tests/{credentials,pool,supervisor,backup,operations}.rs`, `memory-store/tests/migrate.rs` (PostgreSQL-gated), `packages/supervisor/test/supervisor.test.ts` (crash, outage, stale input), `packages/cli/test/backup-restore.test.ts`. Open: dispatch for activation/consolidation/maintenance (process-input contract), credential persistence, live production run. |

## WP16 release evidence

`RELEASE.md` freezes the version matrix, lists every judgement family's active mode and
fallback, and separates operational, semantic and behavioural results. Rows above marked
"unqualified" or "open" are carried into its not-qualified register; §21.1 (the agent
property-investigation story) is the first follow-on WP16 story and is not yet run.
