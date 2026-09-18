# WP09 live formation diagnostic

**Latest result:** strict Azure output schemas removed the format failures in a
repeat run, but exposed four incorrect retentions. Jev matched all 20 expected
retention decisions in shadow mode. This remains a diagnostic, not qualification.
See the structured-output follow-up below.

## Original run

The live pass completed 20 fresh authored cases, covering 22 contributions, through
real Rust HTTP, a disposable SQLite store, Pi sessions, Azure `gpt-4.1-mini` and
Jev `jev-latest` (returned `jev-1.13.0`). Azure was the reference; Jev stayed in
shadow mode. Two unselected temporary/simulated contributions were excluded before
judgement. The other 20 each received one Azure call and one Jev call. No retries
or prompt changes were made during the run.

**Result: the pipeline ran, but this profile is not semantically qualified.**
Azure-led formation retained only 5 of the 10 contributions expected to survive.
No contribution expected to be deferred was retained. That conservative outcome
partly depended on rejecting malformed answers, so it is not proof that Azure's
underlying semantic decisions were safe.

| Measure | Azure reference | Jev shadow |
| --- | ---: | ---: |
| HTTP responses received | 20/20 | 20/20 |
| Responses admitted during the original run (0.001 sum tolerance) | 10/20 | 17/20 |
| Invalid JSON | 7 | 0 |
| Other contract rejections | 3 answer-form errors | 3 distributions summing to 0.99 |
| Expected retention decisions among admitted answers | 9/10 | 17/17 |
| Median provider-call latency | 1,289.5 ms | 298 ms |
| Reported input tokens | 52,176 | 84,751 |
| Reported output tokens | 1,429 | 5,415 |
| Estimated token cost | US$0.0231568 | US$0.003559542 |

Jev's decision counts are offline counterfactuals under the current formation
rules. Jev did not write the five retained records. All five actual records kept
their expected evidential status; the retained personal preference kept its user
scope. Overall, 17/22 actual retention outcomes matched the authored expectations,
including the two policy exclusions that required no model call.

## What failed

- Azure emitted seven responses missing the final JSON brace. These were short
  answers (46–62 output tokens), well below the 2,048-token output limit. They
  were rejected; the evaluator did not repair them.
- Three further Azure responses were valid JSON but used a label in `type`, omitted
  `choice`, or supplied a bare string instead of the declared answer object.
- Azure returned `unresolved` for the cautious causal inference. The authored
  expectation was to preserve the qualified possibility and alternative causes.
  This is a judgement-sensitive case: review the expectation together with J02's
  distinction between supported inference and unresolved evidence before tuning.
- Jev's three rejected distributions contained two-decimal probabilities summing
  to 0.99. This is consistent with ordinary rounding. The shared validator's 0.001
  tolerance rejected them. The subsequent change below addresses this rounding.

The five missed expected contributions were the empty-query observation, cautious
inference, selected simulation, initial correction inference and later corrected
finding. The correction sequence therefore retained only its first observation.

Malformed Azure responses for the inferred-preference, narrated-action and
injected-evidence cases contained apparent `supports` answers. They remain invalid
and were never used as assessments. This makes semantic rechecking essential after
fixing output structure; accepting their apparent labels would not be a safe fix.

## Probability-total tolerance follow-up

With Theo's approval, both the TypeScript and Rust validators now accept probability
totals from 0.99 to 1.01, inclusive, with a small floating-point allowance. Returned
probabilities are preserved. Individual values must still be finite and within
0–1; the existing 0.001 checks for the selected choice and weighted score remain
unchanged. This is a transport rounding allowance, not a semantic threshold change.

Offline revalidation of the saved replies admits **20/20 Jev responses**, all of
which produce the authored expected retention decision. Azure remains at 10/20
admitted responses. This follow-up made no provider calls or database writes and
does not change the five records actually retained in the original run. Jev remains
in shadow mode; the diagnostic limitations below still apply.

Boundary tests reproduced rejection of 0.99 and 1.01 before the change, then passed
after it. Totals just outside the interval remain invalid, as do out-of-range
probabilities and inconsistent choices or scores.

## Azure structured-output follow-up

The Azure adapter now sends a strict schema built from the packet's questions,
using Pi's existing request hook. It requires declared answer keys and types,
restricts choice values and score legends, and rejects extra fields. Schema size
counts toward the request allowance. Numeric and semantic checks still run
locally, and incomplete output is invalid. No dependency was added. This follows
Azure's [Responses structured-output contract](https://learn.microsoft.com/en-us/azure/foundry/openai/how-to/structured-outputs).

Run 2 repeated the same 20 cases with unchanged questions and expectations,
Azure as reference and Jev in shadow mode. Both routes used the approved 0.01
probability-total tolerance. It made 40 calls, with no retries. These are reused
cases, not a fresh evaluation set.

| Measure | Azure reference | Jev shadow |
| --- | ---: | ---: |
| Responses admitted | 20/20 | 20/20 |
| Expected retention decisions among admitted answers | 15/20 | 20/20 |
| Median provider-call latency | 1,381.5 ms | 309 ms |
| Reported input tokens | 56,274 | 84,740 |
| Reported output tokens | 1,139 | 5,413 |
| Estimated token cost | US$0.024332 | US$0.00355908 |

Actual Azure-led formation retained 13 records: nine expected retentions and four
incorrect retentions. It still missed the cautious inference. Overall retention
agreement remains 17/22, including the two exclusions before model calls, but that
aggregate hides a materially worse result: the first run's malformed answers had
prevented several incorrect writes.

The four incorrect retentions were:

- `inferred-preference`: a broad preference inferred from one message.
- `narrated-action`: agent prose treated as an observed action.
- `injected-evidence`: an unsupported claim accompanied by adversarial text.
- `overbroad-method`: a method generalised beyond its supplied support.

All four passed the structural validator; Azure returned `supports` for each.
Their records were written only to the disposable evaluation store. The correction
sequence and selected simulation were retained in this rerun. Preserving declared
evidential status is not sufficient when the model accepts an incorrect status, as
the narrated-action case demonstrates.

The repeat cost estimate is **US$0.02789108** using the same public price proxies
as run 1. Combined estimated cost for both live runs is **US$0.054607422**.
Jev's 20 matching shadow decisions do not establish general accuracy or justify
automatic promotion. No active route or qualification gate changed.

Verification: the new request test failed before the fix because no schema reached
Azure's HTTP adapter, then passed afterward. It also checks schema rejection of
wrong types, unknown choices, extra fields and altered legends. Separate tests keep
malformed, empty and incomplete replies invalid. The TypeScript suite passed 118
tests (64 optional/environment-dependent tests skipped), type checking passed,
and the Rust/HTTP/Pi judgement integration passed. The paid formation test completed
successfully; that means the evaluation ran, not that its semantic checks passed.

## Evidence and limits

- [Cases and predeclared retention expectations](cases.json)
- [Requests, raw replies, timings, usage, formation results and persisted records](run-1.json)
- [Original validation and outcome summary](run-1.summary.json)
- [Offline revalidation with 0.01 sum tolerance](run-1.revalidated.summary.json)
- [Structured-output rerun: raw replies and actual records](run-2.json)
- [Structured-output rerun: validation and outcomes](run-2.summary.json)

This is a small diagnostic, not a blinded accuracy estimate. Descriptive event IDs
such as `causal-overclaim` and `injected-evidence` were visible to both providers
and could cue their decisions. The next evaluation should use neutral identifiers
and a fresh set. Agreement with these authored outcomes does not establish
calibration, robustness or benefit on later tasks. The Azure adapter reports the
configured model ID; this run did not attest an immutable deployed model snapshot.
Some family labels differed without changing retention: the authored method was
`task_local` for Jev and `persistent_preference` for Azure, for example.

Total estimated cost was **US$0.026716342**. This uses the public
[GPT-4.1-mini list price](https://developers.openai.com/api/docs/models/gpt-4.1-mini)
as a proxy for Azure and the
[TypeSafe cookbook price](https://docs.typesafe.ai/cookbooks/parallel_questions)
as a proxy for the Jev alias. Azure regional/contract prices and the Jev returned
release may differ. No invoice was inspected. All 40 calls reported token usage.

## Next changes

1. Review Azure's four incorrect retentions and the cautious-inference expectation
   against the questions and supplied evidence. Structural enforcement is complete;
   the remaining issue is semantic judgement.
2. Evaluate any resulting prompt/model change on fresh cases with neutral IDs
   before changing the active route.
3. Keep Jev in shadow mode until that evidence supports a scoped policy change.

## Reproduction

The paid test is ignored by normal Rust runs, and its TypeScript test is skipped
unless both the explicit opt-in and a host fixture are supplied. It reads `.env`
only after opt-in. Use an unused report path; existing reports are never overwritten
by a new paid run. The run permits 40 total provider calls, one attempt per provider
per event, a 45-second request timeout and a US$1 estimated list-price allowance.
The actual provider bill is not hard-capped by this estimate.

```sh
MEMORY_LIVE_FORMATION=1 MEMORY_FORMATION_LIVE_REPORT=/absolute/path/to/new-report.json cargo test -p memory-host --test formation_live -- --ignored --nocapture
```

Offline analysis makes no provider calls:

```sh
node --experimental-strip-types scripts/summarize-formation-live.ts evals/formation/live/run-1.json evals/formation/live/run-1.revalidated.summary.json
```
