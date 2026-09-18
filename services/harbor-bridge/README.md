# Harbor and ASP execution

The bridge owns sandbox provisioning, recovery, export and deletion. Pi uses
`AspExecutionEnv` for remote file and shell operations over SSH. The same lifecycle
runs on local Docker and the bounded Daytona qualification profile. There is no
local tool fallback when a remote endpoint fails.

## Allocation and recovery

`Lifecycle` checks the current Rust assignment, reserves sandbox resources and
prepares a coordinator effect before asking Harbor to create an allocation. The
allocation ID and provider labels let it find the same sandbox after a lost reply.
The successful effect stores the binding receipt. A missing previously successful
allocation is reported as lost; it is never silently replaced.

A private SQLite work queue retains incomplete allocation, export and deletion
work. One OS lock owns that directory. `reconcile` checks it against the host lease
and provider state, retries unfinished cleanup, and removes expired orphaned
allocations bearing this bridge's labels. This is a local owner, not a distributed
provisioning service. The WP15 supervisor will invoke reconciliation periodically.

Harbor's `attach()` opens an interactive shell; it is not used for recovery.
Docker recovery uses provider labels and reconstructs the Harbor environment.
Daytona uses SDK lookup and a small compatibility subclass for the pinned Harbor
version, whose public API lacks reconnect, unique-name and TTL parameters. Harbor
can swallow deletion failures, so both providers verify absence afterwards.

## Execution and ownership

The worker accepts only a descriptor matching its trusted allocation receipt. It
pins the SSH host key, disables agent forwarding and ambient SSH configuration,
and uses ASP's exec fallback for file transfer. SFTP is not implemented.

The helper and binding are root-owned. Generated commands run as UID/GID 1000.
File calls are bounded to 1 MiB, directory lists to 4,096 entries, and retained shell
output to 16 MiB. Reads can be chunked. Each shell operation has an execution ID
and a detached supervisor. Cancellation stops its process group and verifies that
it is gone. A separate lease watchdog kills all UID 1000 processes on expiry,
including children that leave their original process group.

Lease renewal updates the protected binding and the worker's receipt. During
export, only the host lease is renewed; the execution endpoint stays fenced. An expired
or changed owner cannot continue issuing helper operations. Docker uses a forced
SSH command and a private per-allocation key. Daytona uses a short-lived gateway
token retained only in trusted bridge/worker state; generated code does not receive
it or the provider API key.

## Exports and resource accounting

The caller declares up to 16 workspace files with artifact IDs and byte allowances
(up to 16 MiB each). Before job completion, the worker invokes `finish`: stop
sandbox work, download those files, publish them through the host artifact API,
then delete the provider allocation. A completed download survives a lost upload
reply. Publication failure keeps cleanup pending. Cancellation or proven sandbox
loss can record a missing output as unavailable; it cannot turn a host outage into
permission to discard an existing export.

The host exposes administrative `allocate_artifact` and `recover_artifact_upload`
commands, `PUT /v1/artifacts/{id}` (16 MiB), and scoped bounded GET reads (1 MiB).
Published artifacts cannot be overwritten. Worker credentials cannot use these
administrative upload endpoints. The bridge credential must use the assignment's
actor and scope, with the administrator role; it stays outside Pi context.

Sandbox time, CPU and declared output capacity are reserved before allocation.
The Daytona test profile reserves its full 15-minute lifetime and US$0.10 per allocation. Neither provider
currently supplies authoritative CPU/billing settlement to this bridge, so usage
remains unknown and retains the maximum allowance. These reservations are not a
claim about invoiced cost.

## Run locally

Install the pinned Harbor package and its Daytona extra in the project environment:

```sh
uv venv --python 3.13 .venv-harbor
uv pip install --python .venv-harbor/bin/python -r services/harbor-bridge/requirements.txt
npm run test:bridge
.venv-harbor/bin/python -m unittest discover -s services/harbor-bridge -p 'test_*.py'
npm run test:asp
npm run test:docker
```

`test:docker` builds `memory-asp:wp05` and runs the actual Rust host, Python bridge
and Pi native tools. Set `DOCKER_HOST` for an existing engine, or use the isolated
`memory-wp05` Colima profile. Restart a stopped profile with `colima start memory-wp05 --activate=false`.
The script does not change the default Docker context.
The Docker profile enforces CPU/memory/PID limits, a loopback SSH port and outbound
firewall rules. Only private Harbor trial directories are mounted; the repository
and `.env` are not mounted.

`cli.py /absolute/path/bridge.json` accepts one JSON command on stdin: `create`,
`renew`, `finish` or `reconcile`. The configuration must have mode 0600 and contains
`directory`, `bridge_id` (UUID), `host_url`, `host_token`, and `provider` (`docker`
or `daytona`). Docker optionally accepts `docker_host` and `docker_config`.
Daytona accepts `env_file` and `gateway_key` (the approved SSH public key). Only
`DAYTONA_API_KEY` is read from that file. Keep the state directory and configuration
private, and retain them until all allocations and exports have settled.

## Daytona qualification profile

The profile uses 1 CPU, 1 GiB memory, 3 GiB disk, blocked outbound traffic, five-minute
idle stop, deletion on stop and a 15-minute provider TTL. It is a short test profile,
not a general long-running deployment. The endpoint lease remains independently
fenced. The provider must accept the requested network policy; failure does not
fall back to unrestricted networking.

```sh
npm run test:daytona -- --budget-usd 5 --gateway-key-file /path/to/approved-gateway-key
```

This command uses `.env` and creates paid resources. It reserves a conservative
US$0.25 test allowance per invocation in `.runtime/daytona-test-spend.json`. This is
an estimate and a local retry limit, not a provider billing cap. Current published
[Daytona rates](https://www.daytona.io/pricing) imply under US$0.02 for this profile's
full 15-minute lifetime, excluding build time. Check actual billing separately.

On 17 September 2026 the public API's `sshGatewayPublicKey` was empty. Theo approved
first-connection trust for the observed `ssh.app.daytona.io` key; it is pinned in
`.runtime/daytona-gateway.keys` for these tests. A deployment needs its own trusted
key or a nonempty key from Daytona's HTTPS configuration. The adapter never
silently accepts an unverified or changed key. See Daytona's
[SSH](https://www.daytona.io/docs/en/ssh-access/) and
[network policy](https://www.daytona.io/docs/en/network-limits/) documentation.

## Evidence and limits

Docker has passed the complete host → Harbor → Pi/ASP → artifact → deletion path,
including same-allocation recovery, unprivileged execution, blocked outbound
traffic, protected binding files and expiry of escaped child processes. Unit tests
cover lost replies, publication failures, missing output, stale ownership and
cleanup retries. The shared integration also exercises lease renewal.

The same complete integration passed on Daytona (SDK 0.214.0, API v0.214.3). Its
egress proxy accepted TCP establishment but rejected the TLS/data exchange. The
test checks actual data transfer, rather than relying on the connection result or
policy flag alone. A provider-list race during deletion was reproduced and fixed;
a direct not-found result confirms absence. No test sandboxes remained in the final
filtered provider listing. Six test invocations reserved US$1.50 of the approved
US$5 allowance; that allowance is not an invoice or a measurement of actual spend.

Production deployment, distributed ownership,
live model calls, provider billing reconciliation and continuous cleanup supervision
remain outside this increment. The [plan](../../docs/implementation/plan.md) records
the current acceptance status.


## Scoped interpreter

The image and bridge bootstrap install `interpreter.py` beside the protected ASP
helper. The `interpreter` operation first checks the sandbox binding and runs as
the unprivileged execution user. It maintains a bounded Python process in the
workspace, with selected JSON checkpoints and execution receipts. The outer lease
watchdog and provider teardown still apply to that process.

`npm run test:docker` also exercises the Pi `python` tool, persistent objects,
checkpoint publication and restoration into a new interpreter. `npm run test:asp`
checks repeated interpreter calls through real OpenSSH. Python tests exercise
wall timeouts, lost heaps, uncertain receipts and serialization limits. See the
[runtime guide](../../docs/implementation/runtime.md) for the caller API and
recovery rules. The updated image has been tested locally; WP07 does not extend
the earlier live Daytona qualification.
