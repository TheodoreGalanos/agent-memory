# Agent Memory — System Specification v0.2

Conceptual design · 17 September 2026

## Contents

- `specification.md`: complete high-level system specification.
- `CHANGELOG.md`: historical resolution map for the earlier document review, with page references and a diagram index.
- `diagrams/*.svg`: seven editable, original vector diagrams.

The specification describes two logical stores, four peer processes, four retained-memory families, shared context and policy, and three operating loops. It includes workspace lifetimes, bounded and cache-aware rendering, entity-centred retrieval, explicit memory citations, bitemporal claims, scheduled conflict investigation, memory-owned intentions and revision scoped to relevant unfinished work.

The host harness supports scoped operations over referenced working material. Each operation has a work brief, bounded context, evidence access, an aggregate resource budget and an inspectable result. The specification describes how findings return to the main task, how evidence remains traceable across operations, and how to compare execution strategies. Recursive Language Models and code execution with MCP inform this design; their primary sources are references [12] and [13].

Section 12 defines process testing and evaluation. It separates unit and contract tests, semantic evaluations and behavioural evaluations. A shared fixture structure supports tests of the four memory processes, workspace and harness behaviour, caching, result reuse and the evaluator itself. The existing scenarios form a common library for isolated tests and longer task histories.

Interaction responsibilities run through the same architecture. Users can inspect the basis of work, make scoped corrections, teach from examples, control exploration and transfer, and manage commitments and notifications. Routine operations continue under policy. Requests for user input identify the decision needed and what happens while it remains unanswered. Interaction scenarios assess both effective user influence and successful autonomous work.

Appendix A defines the shared semantic-judgement capability, with Jev as the provider being evaluated. It covers evidence packets, process-specific uses, uncertainty, policy responses, temporary checks, provenance, reuse and adoption criteria. Figures A1 and A2 use the same visual conventions as the main figures; Figure 1 also names the three harness capabilities. The appendix keeps its own source references, S1–S7, alongside the existing references 1–13.

Open `specification.md` in a Markdown viewer that supports local SVG images. Keep the `diagrams` directory alongside it so the relative image links resolve. The SVGs contain editable shapes, connectors and text, with descriptive titles and alternative descriptions. They use Lato with Arial and sans-serif fallbacks; no font files are included.

The companion PDF, `../agent_memory_system_specification_v0.2.pdf`, contains the same specification text and rendered figures in a 40-page layout. Edit the text in `specification.md` and the diagrams in the SVG files. The earlier Word edition is not included in this folder.
