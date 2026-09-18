# Shared contracts

Edit the types in `crates/memory-domain/src/contracts.rs` or `coordination.rs`,
then run `npm run generate`.
The generated JSON Schemas and TypeScript types are saved project files;
they are not separately maintained definitions. `work-command` wraps a `WorkBrief`.
The other schemas describe `WorkResult`, assignments, the common response,
the host command request/response unions, and workspace state/render manifests.
Workspace transitions are owned by the TypeScript Pi worker; its checkpoint API
validates scope, lineage, conflicts and lifetimes before persistence.

Domain identities are UUIDs (new records will use UUIDv7); memory references use an
integer revision and a readable label. Labels do not establish identity. Pi IDs
remain opaque strings. Dates serialize in UTC. Integer bounds fit JavaScript
numbers; costs use integer minor units and a currency code.

The schemas validate the wire shape. Rust methods additionally check the declared
scope, command/brief agreement, deadline, and result coverage. The host must supply
`Authority` from authenticated state, call both `check_authority` and `validate`,
and resolve every referenced object under that authority. The domain crate contains validation; `memory-host` supplies authenticated
authority and `memory-store` resolves references and ownership.

Scope restrictions can be narrowed but not omitted to gain access. User, project,
task, entity and source revision remain separate dimensions. A complete result
cannot conceal unresolved coverage or an unknown effect. Pi's completed operation
status remains distinct from the application's complete/partial/blocked result.

The response reason codes distinguish invalid input, authority, revision/command
conflicts, deadlines, budgets, evidence gaps, unavailable dependencies, cancellation
and unknown effects. `POST /v1/commands` consumes `HostRequest` and returns
`HostResponse`. `HostClient` uses these generated types. `/openapi.json` describes
the API and links its served JSON schemas; see the [runtime guide](../docs/implementation/runtime.md).

`compatibility.json` pins reviewed upstream sources. `upstream/asp.schema.json` is
the separately licensed ASP connection schema, not a project execution adapter.
