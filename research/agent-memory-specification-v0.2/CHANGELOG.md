# Changes from v0.1 to v0.2

**17 September 2026 · Conceptual system specification**

This is the historical resolution map for the original v0.2 review. The subsequent sharing edit simplifies the prose, removes internal review references and the future decision-value policy discussion, and adjusts two labels in Figure 1. Later additions clarify workspace lifetime, caching, retrieval, investigation and scoped revalidation, then add harness-supported scoped execution, aggregate budgets and evidence lineage across operations. Figures 1, 3 and 5 now show these execution paths. Section 12 has since been expanded into process testing and evaluation, retaining the shared scenario library. Further additions define user inspection, scoped contributions, commitments and communication, with users and authorised operators shown in Figure 1. Appendix A now defines shared semantic judgement and the proposed Jev integration, with two diagrams and seven official source references. The main text links the capability to process, policy, workspace and evaluation responsibilities. Page references, reference numbers and resolutions below describe the earlier edition; the current specification is authoritative.

Version 0.2 separates state from process, names policy and lifecycle owners, makes memory use observable, and connects retained-memory changes to active work. The PDF and Word versions contain 15 pages and five diagrams. The specification remains a description of responsibilities, information relationships and behaviour.

## Review resolution map

| Review point | v0.2 resolution | Location in v0.2 |
| --- | --- | --- |
| Consolidation nested in maintenance | Two logical stores and four peer processes. Consolidation develops abstractions, candidate methods and scope extensions; maintenance curates support, validity and availability. Each has distinct triggers, budgets and review criteria. | At a glance, p.1; composition, p.2; four processes, p.6; rationale, p.14. |
| Policy ownership and diagram placement | The shared band names context and policy. The memory system owns operational policy decisions; the operator configures constraints and budgets, the agent proposes, and the host enforces. Decisions retain their policy version and reason. | Figure 1, p.2; shared policy, p.7. |
| Workspace capacity and rendering | Workspace rendering accounts for a finite host-provided context budget, required task material, response capacity and candidate competition. Selected memory versions, deferred material and origins are recorded. | Workspace, p.4; review scenarios, p.13. |
| Available, selected and applied | Available means eligible candidates; selected means included in rendered context; applied means explicit memory-ID/version citations with a use role. Executable use also has host result evidence. | Observable use, p.7. |
| Application and derivation | Cited inputs establish derivation for new retained representations. Declared use, invocation evidence and measured benefit have separate meanings. | Observable use, p.7; evaluation, pp.12–13. |
| Claim-to-claim disagreement | Added symmetric **conflicts with**, scoped to incompatible positions within overlapping scope and valid time. Relevant conflicts are surfaced with both claims, support and a linked investigation. | Workspace, p.4; vocabulary and Figure 2, p.5; revision, p.10. |
| Bitemporal history | Explicit **valid time** and **transaction time**, with retained prior versions and a late-arriving-update example. Observation time remains source metadata. | Connected memory and time, p.5; review scenario, p.13. |
| Intention ownership and expiry | Memory owns pending, armed, fired, completed, expired and cancelled. Formation creates pending records with default expiry; maintenance arms and records terminal outcomes; activation records firing. The host emits execution and time events. | Intention lifecycle and Figure 4, p.9. |
| Intention edge cases | Expiry and cancellation are reachable from every open state. Defined behaviour for repeated trigger delivery, attempts/retries, late results, renewed commitments and recurring occurrences. | Intention lifecycle, p.9; worked example, p.11; review scenarios, p.13. |
| Executable skills versus advisory playbooks | Both are procedural memory with applicability conditions. Executable checks assess operations and outcomes; advisory checks assess evidence and reasoning criteria. | Memory functions, p.3. |
| Mattar and Daw as a value-model direction | Explicit initial policy rules and budgets are distinguished from a future decision-value model evaluated against that baseline. | Policy, p.7; rationale, p.14. |
| Figure 1 missing feedback and priority paths | Added cited-use/outcome/cost feedback into maintenance and maintenance-to-activation priority/change-event paths. Consolidation is visibly a peer process. | Figure 1, p.2. |
| Original Figure 4 lane label | Source-revision sequence now names **Memory collection + processes** and shows bounded rendering, cited use, checked completion and active-workspace reconciliation. | Figure 5, p.11. |
| Maintenance during active work | Committed revisions publish versioned change events through activation. Each workspace reconciles explicitly; the host checks required current dependencies before consequential actions and coordinates concurrent effects. | Revision and active work, p.10; Figure 5, p.11; review scenarios, p.13. |
| Deleted episode versus surviving claim | Governed content removal follows derivative content. Separately retainable claims are reassessed against remaining independent support; claims below policy support are retired and linked to revalidation where useful. Provenance markers obey the deletion scope. | Deletion and support reassessment, p.10; review scenarios, p.13. |
| Preference scope and authority | Explicit user statements establish the user’s stated preference within its scope and time. Project-specific and inferred preferences retain their distinctions. | Memory functions, p.3. |
| Reference additions and Hindsight authors | Added Generative Agents [11] for reflection, cited inputs and scoring; A-MEM [12] for dynamic linking. Hindsight [8] now lists all seven authors and the submission date, verified against its arXiv record. | Rationale and references, pp.14–15. |

## Diagram index

| Figure | File | Page |
| --- | --- | --- |
| 1. System composition | `diagrams/01-system-composition.svg` | 2 |
| 2. Connected memory | `diagrams/02-memory-relationships.svg` | 5 |
| 3. Operating loops | `diagrams/03-operating-loops.svg` | 8 |
| 4. Intention lifecycle | `diagrams/04-intention-lifecycle.svg` | 9 |
| 5. Source revision | `diagrams/05-source-revision-sequence.svg` | 11 |

## Retained strengths

The revision preserves the functional families; the distinction between knowledge and its constituent claims; attributed sources and responsible owners; independent reliability, applicability and utility assessments; costs in feedback; derived-content deletion; and behaviour-to-evidence review criteria. The adaptive-attention criterion continues to require improved outcomes or lower cost at comparable quality.

## Clarifications carried into the revision

Memory citations record declared application. Execution receipts add evidence of invocation, while comparative evaluation establishes whether the memory improved the result.

Both procedural forms have meaningful checks. Their checks follow their form: execution and observable results for skills; evidence and judgement criteria for advisory playbooks.

Expiry and cancellation apply throughout the open intention lifecycle. Terminal records retain subsequent result evidence, and renewed work receives an explicit follow-up commitment.
