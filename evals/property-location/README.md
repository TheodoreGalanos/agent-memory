Three checkpoints follow the conceptual property-location example:

- **Before correction:** an instance query is empty. The property might still be on the type.
- **After correction:** schema inspection and a type query establish its location in C.
- **Revision D:** the historical C finding is available, but D has not been inspected.

`inputs/` contains only the command and evidence visible to a worker at that checkpoint.
`reference-world.json` contains evaluator-only facts, including D's eventual answer.
`results/` contains scripted provider responses for plumbing tests. Neither evaluator
file is put in the worker's prompt or execution directory. The checkpoint harness
enables only `read`, and a test hook restricts reads to the checkpoint evidence
file. A negative test attempts to read the evaluator world and observes a blocked
tool result. This fixture restriction is not a production sandbox implementation.

The tests check scope, evidence cutoff, status and preservation of meaning. They do
not grade sentence wording. Scripted responses demonstrate contract and harness
behaviour, not formation quality or live model reasoning. WP09 and WP16 will use
these checkpoints to test those behaviours.
