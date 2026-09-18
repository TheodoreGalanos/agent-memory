# Judgement fixtures

`fixtures.json` contains one authored example for each of J01–J29, with named
material and scripted expected answers. The contract tests also exercise explicit
missing evidence and invalid provider output for every family. These fixtures
verify the packet/answer/disposition/fallback path, not a model's accuracy.

Question definitions live in `packages/judgement/src/catalogue-data.json`.
Definitions carry their own criteria, applicability, permitted uses and evaluation
references. Compound families contain separate questions. Choice lets these
fixtures distinguish yes, no and insufficient evidence; Noul and Score have
separate primitive validation tests.

Run `npx vitest run packages/judgement/test/contract.test.ts
packages/judgement/test/packet.test.ts` for the local contract checks. Run
`cargo test -p memory-host --test judgement` for the Rust HTTP, artifact, budget,
Pi session and scripted provider integration. `npm run test:store` repeats that
integration against PostgreSQL. Provider timeout tests require local loopback.

All 29 families remain semantically unqualified. Before enabling a real Jev
qualification, add representative held-out positive, negative, missing-evidence,
ambiguous and conflicting cases for the target domain. Compare both answer
quality and downstream policy errors against the conventional route. Evaluate
the release and definition revision together, choose a threshold from those
results, and retain the report as the qualification artifact. Do not use the
scripted examples or TypeSafe's published benchmark as that evidence.

The 18 September live smoke report is `live-smoke.json`. It retains the rejected
cookbook model request and three successful `jev-latest` calls (returned model
`jev-1.13.0`). Batched and separate-family labels agreed for all three questions;
only one matched the authored expected label. J01/J02 revision 2 addresses the
criteria ambiguity exposed by that run. The separate `live-smoke-revised.json`
report tests revision 2 on the same example and returned all three expected labels
in both batch and separate-family calls. Both runs reported `jev-1.13.0`. Neither
report is held-out qualification evidence.

The opt-in test `live-jev.test.ts` reads `JEV_API_KEY` only when
`MEMORY_TEST_JEV=1`. It refuses another run once this smoke report's allowance
is used. A fresh paid evaluation needs a deliberate new allowance and report. Set
`MEMORY_JEV_REPORT` to its filename under `evals/judgement` to preserve earlier
results; the default remains `live-smoke.json`.
Ordinary repository checks neither load `.env` nor call Jev.
