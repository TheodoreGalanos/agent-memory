# Campaign scaffold

A campaign runs a benchmark's task list against the pool under three isolated arms
and reports whether the memory system helps, without ever having to trust its own
claims about itself.

## Arms

- **no_memory** — no memory process is ever enabled; every task starts from
  nothing and answers from first principles every time.
- **capture_only** — formation is enabled (ordinary task experience gets
  captured into candidate memories) but nothing consolidates, activates, or
  maintains what was captured.
- **full** — every learning process is enabled: formation, consolidation,
  evaluation, activation, maintenance.

Each arm runs against its own freshly created Host and database, so one arm's
memory can never leak into another's comparison.

## Transfer tasks

A task list is a mix of **seed** tasks and **transfer** tasks (`transfer: true`).
Seed tasks run first, in the same project scope, giving a memory process (in
arms where one is enabled) something to have formed from. Transfer tasks run
afterward and are the actual measurement: whether having seen the seed tasks
changed the outcome. Comparing a transfer task's `full` outcome against its
`no_memory` outcome is where memory's behavioural value shows up — never a
seed task's own outcome, since nothing has had a chance to form yet when a
seed task runs.

## The adapter seam

A campaign is not benchmark-specific. `packages/supervisor/src/adapter.ts`'s
`TaskAdapter` already separates what a task may see and do from an independent
check of its settled result; that is exactly the seam a held-out benchmark
needs. To run a real benchmark (AEC-Bench or otherwise), define one
`TaskAdapter` per task family (or reuse the deterministic default,
`criteriaAdapter`, for output-criteria-only tasks) and supply a `CampaignTask[]`
naming it. Nothing in `run.ts` needs to change; the campaign only orchestrates
arms, transfer holdout, and reporting.

## Outcomes reported

Every task, in every arm, reports three separate things — deliberately kept
apart, because a task can be operationally fine while being semantically
wrong, or vice versa:

- **Operational correctness** — did the job settle at all (`operational:
  "settled" | "error"`), independent of whether the answer was any good.
- **Semantic quality** — the task's own `TaskAdapter.validate()` verdict
  (`"passed" | "failed" | "unknown"`) against the brief's output criteria,
  never inferred from the result's own claimed status.
- **Behavioural value** — for each transfer task, whether the `full` arm both
  completed more fully and cost no more than the `no_memory` arm ran the same
  task for. This is the only place arms are compared against each other; every
  other report field is per-arm, per-task.

Process cost is the Host's own `budget_usage` for the arm's shared budget (real
provider-call reservations, when a live task adapter makes them); a
fixture-driven dry run has no real reservations to report, so it falls back to
summing each outcome's own declared cost instead.

## Dry run (no credentials, no cost)

```
npm run --silent inventory > /dev/null  # not required; just confirms Node/tsx wiring works the same way
node --import ./scripts/ts-loader.mjs evals/campaign/run.ts
```

runs `evals/campaign/fixtures/dry-run-tasks.json` (3 fixture tasks, one seed and
two transfer) through all three arms against a real, local, freshly created
Host each time — no live model, no credentials, no network egress. It exists
to prove the campaign's own orchestration (arms, transfer holdout, adapter
checking, cost reporting), not to evaluate a real benchmark.
`packages/supervisor/test/campaign-dry-run.test.ts` runs this in the ordinary
test suite.

## Live campaigns (paid — gated)

A real campaign against a real benchmark makes real provider calls and costs
real money. Nothing in this repository runs one automatically:
`MEMORY_LIVE_CAMPAIGN=1` must be set, and the caller supplies its own real task
list (with adapters) and provider credentials — there is no default live task
list checked in. Running `node evals/campaign/run.ts` directly with
`MEMORY_LIVE_CAMPAIGN=1` and no further wiring refuses, on purpose, rather than
silently falling back to the dry-run fixtures.
