# Campaign reporting fixture

This directory tests campaign bookkeeping with supplied outcomes and costs.
It starts a separate local Host and database for each of three labels:
`no_memory`, `capture_only` and `full`. It submits and completes jobs through the
API, runs seed tasks before transfer tasks, and compares the supplied results.

**It does not launch worker pools, enable different memory processes, or run a
benchmark.** Differences between arms come from the fixture JSON. They are not
measurements of learning, model quality or savings from memory.

## Run it

After `npm run setup` and `npm run build`:

```sh
node --import ./scripts/ts-loader.mjs evals/campaign/run.ts
```

This runs the three tasks in `fixtures/dry-run-tasks.json` across all three arms.
No provider credentials or paid services are needed. The corresponding test is:

```sh
npx vitest run packages/supervisor/test/campaign-dry-run.test.ts
```

The report includes job settlement, the task adapter's verdict, fixture costs,
and comparisons for transfer tasks. The default `criteriaAdapter` checks whether
reported coverage or findings contain each output criterion. It does not verify
the underlying work. Supplying a different adapter changes validation only.

## What a real campaign still needs

Live execution needs a driver that launches each worker composition, supplies
benchmark inputs, waits for memory processing, checks task outputs independently,
and measures provider usage. `runFixtureCampaign` does not implement that driver.
The entrypoint rejects `MEMORY_LIVE_CAMPAIGN=1` instead of returning fixture
results as live evidence.

The pool's actual composition and learning tests are in
`packages/supervisor/test/compositions.test.ts` and `learning.test.ts`.
They exercise the runtime with test providers; they also do not qualify model
quality or demonstrate a benefit on a real benchmark.
