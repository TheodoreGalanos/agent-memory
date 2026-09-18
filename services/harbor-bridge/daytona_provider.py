"""Daytona qualification profile using the pinned Harbor lifecycle and SDK lookup."""

import asyncio
import base64
import json
import re
import shlex
import shutil
import time
import uuid
from pathlib import Path
from urllib.request import urlopen

from bridge import AllocationRequest, HarborAllocation
from control import export_file, fence_processes, renew_binding
from daytona import ListSandboxesQuery
from daytona.common.errors import DaytonaNotFoundError
from harbor.environments.daytona import DaytonaClientManager, DaytonaEnvironment
from harbor.models.task.config import EnvironmentConfig, NetworkPolicy
from harbor.models.trial.paths import TrialPaths

OWNER = "io.agent-memory.bridge"
LABEL = "io.agent-memory.allocation"


class RecoverableDaytonaEnvironment(DaytonaEnvironment):
    # Harbor 0.22 has no noninteractive reconnect or name/TTL constructor fields.
    # Keep the three pinned implementation hooks here; all file/process work still
    # uses Harbor's public interface. The live qualification covers these hooks.
    def _image_sandbox_params(self, **kwargs):
        params = super()._image_sandbox_params(**kwargs)
        params.name = self.session_id
        params.ttl_minutes = 15
        return params

    async def exec(self, command, cwd=None, env=None, timeout_sec=None, user=None):
        # Harbor's session poll has no outer deadline, even after remote timeout.
        return await asyncio.wait_for(
            super().exec(command, cwd=cwd, env=env, timeout_sec=timeout_sec, user=user),
            (timeout_sec or 30) + 20,
        )

    async def reconnect(self, sandbox):
        self._client_manager = await DaytonaClientManager.get_instance()
        self._sandbox = sandbox


class DaytonaProvider:
    reservation_time_ms = 900000
    reservation_cost_microunits = 100_000

    def __init__(self, directory, owner, gateway_key=None):
        self.directory = Path(directory).resolve()
        self.directory.mkdir(parents=True, exist_ok=True, mode=0o700)
        self.owner = str(uuid.UUID(owner))
        self.gateway_key = gateway_key

    async def close(self):
        # Close on the owning event loop, before Harbor's atexit fallback runs.
        await (await DaytonaClientManager.get_instance())._cleanup()

    async def client(self):
        return await (await DaytonaClientManager.get_instance()).get_client()

    async def sandboxes(self):
        client = await self.client()
        return [
            s
            async for s in client.list(
                ListSandboxesQuery(labels={OWNER: self.owner}), request_timeout=20
            )
        ]

    async def lookup(self, identifier):
        matches = [
            s for s in await self.sandboxes() if s.labels.get(LABEL) == identifier
        ]
        if len(matches) > 1:
            raise RuntimeError("Multiple Daytona allocations require reconciliation")
        if not matches:
            return None
        try:
            return await (await self.client()).get(matches[0].id, request_timeout=20)
        except DaytonaNotFoundError:
            # List results can lag a confirmed deletion. Direct lookup wins.
            return None

    async def allocations(self):
        return [
            {
                "id": s.labels[LABEL],
                "provider_id": s.id,
                "fence": {
                    "job_id": s.labels["io.agent-memory.job"],
                    "owner_id": s.labels["io.agent-memory.owner"],
                    "epoch": int(s.labels["io.agent-memory.epoch"]),
                },
            }
            for s in await self.sandboxes()
        ]

    def environment(self, record):
        request = record["request"]
        if request["cpu"] != 1 or request["memory_mb"] != 1024:
            raise ValueError("Daytona qualification uses 1 CPU and 1024 MiB")
        if request["expires_at"] > (time.time() + 15 * 60) * 1000:
            raise ValueError("Daytona qualification is limited to 15 minutes")
        root = self.directory / record["id"]
        environment = root / "environment"
        environment.mkdir(parents=True, exist_ok=True)
        source = Path(__file__).parent
        shutil.copyfile(source / "image/Daytona.Dockerfile", environment / "Dockerfile")
        shutil.copyfile(source / "asp_helper.py", environment / "asp_helper.py")
        shutil.copyfile(source / "interpreter.py", environment / "interpreter.py")
        shutil.copyfile(
            source / "image/lease_watchdog.py", environment / "lease_watchdog.py"
        )
        assignment = request["assignment"]
        return RecoverableDaytonaEnvironment(
            environment_dir=environment,
            environment_name="memory-asp",
            session_id="memory-" + record["id"],
            trial_paths=TrialPaths(root / "trial"),
            task_env_config=EnvironmentConfig(
                cpus=1, memory_mb=1024, storage_mb=3072, build_timeout_sec=180
            ),
            network_policy=NetworkPolicy(network_mode="no-network"),
            labels={
                OWNER: self.owner,
                LABEL: record["id"],
                "io.agent-memory.job": assignment["job"]["id"],
                "io.agent-memory.owner": assignment["owner_id"],
                "io.agent-memory.epoch": str(assignment["epoch"]),
            },
            auto_stop_interval_mins=5,
            auto_delete_interval_mins=0,
        )

    async def attached(self, record):
        sandbox = await self.lookup(record["id"])
        if sandbox is None:
            raise FileNotFoundError("Daytona allocation is missing")
        env = self.environment(record)
        await env.reconnect(sandbox)
        return env, sandbox

    async def host_key(self):
        if self.gateway_key:
            key = self.gateway_key
        else:

            def fetch():
                with urlopen(
                    "https://app.daytona.io/api/config", timeout=15
                ) as response:
                    return json.loads(response.read(256 * 1024))["sshGatewayPublicKey"]

            key = await asyncio.to_thread(fetch)
            if key and not key.startswith(("ssh-", "ecdsa-")):
                key = base64.b64decode(key).decode().strip()
        if not key or not re.fullmatch(
            r"(?:ssh-ed25519|ssh-rsa|ecdsa-sha2-nistp256) [A-Za-z0-9+/]+={0,3}", key
        ):
            raise RuntimeError(
                "Daytona did not publish a gateway host key; configure a trusted gateway_key"
            )
        return key

    async def allocate(self, record):
        key = await self.host_key()
        prior = await self.lookup(record["id"])
        env = self.environment(record)
        request = record["request"]
        assignment = request["assignment"]
        owner = AllocationRequest(
            record["id"],
            record["id"],
            assignment["job"]["session_id"],
            assignment["epoch"],
            request["expires_at"],
            "/workspace",
            "ssh.app.daytona.io",
            22,
            "",
            1000,
            1000,
        )
        allocation = HarborAllocation(env, owner)
        if prior:
            if prior.state.value != "started":
                raise RuntimeError(
                    "Existing Daytona sandbox is stopped; do not restart execution"
                )
            await env.reconnect(prior)
            result = await env.exec(
                "cat /run/memory-asp/binding.json", user="root", timeout_sec=10
            )
            if result.return_code:
                raise RuntimeError(
                    "Daytona binding setup was interrupted; cleanup is required"
                )
            binding = json.loads(result.stdout)
            if any(
                binding[k] != getattr(owner, k)
                for k in ("sandbox_id", "generation", "session_id", "epoch")
            ):
                raise RuntimeError("Daytona sandbox belongs to another assignment")
            await renew_binding(env, record, request["expires_at"])
            descriptor = await allocation.inspect_binding()
        else:
            descriptor = await allocation.start()
        sandbox = await self.lookup(record["id"])
        if (
            sandbox is None
            or sandbox.cpu != 1
            or sandbox.memory != 1
            or not sandbox.network_block_all
        ):
            raise RuntimeError(
                "Daytona resource or outbound-network policy does not match"
            )
        # A root-owned watchdog stops escaped children even when the bridge is down.
        watchdog = await env.exec(
            "pgrep -f '^python3 /opt/memory/lease_watchdog.py$' >/dev/null || python3 -c "
            + shlex.quote(
                "import subprocess; subprocess.Popen(['python3','/opt/memory/lease_watchdog.py'],start_new_session=True,stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)"
            ),
            user="root",
            timeout_sec=10,
        )
        if watchdog.return_code:
            raise RuntimeError("Cannot start sandbox lease watchdog")
        access_path = self.directory / record["id"] / "ssh-access.json"
        if access_path.exists():
            access = json.loads(access_path.read_text())
        else:
            token = await sandbox.create_ssh_access(
                expires_in_minutes=15, request_timeout=15
            )
            access = {"token": token.token, "command": token.ssh_command}
            access_path.write_text(json.dumps(access))
            access_path.chmod(0o600)
        if shlex.split(access["command"]) != [
            "ssh",
            access["token"] + "@ssh.app.daytona.io",
        ]:
            raise RuntimeError("Unsupported Daytona SSH gateway address")
        descriptor["descriptor"]["connection"] = {
            "host": "ssh.app.daytona.io",
            "port": 22,
            "user": access["token"],
            "host_key": key,
        }
        return descriptor

    async def renew(self, record, expires_at):
        env, _ = await self.attached(record)
        await renew_binding(env, record, expires_at)

    async def fence(self, record):
        if await self.lookup(record["id"]):
            env, _ = await self.attached(record)
            await fence_processes(env, record)

    async def export(self, record, path, destination):
        env, _ = await self.attached(record)
        await export_file(env, path, destination)

    async def delete(self, record):
        sandbox = await self.lookup(record["id"])
        if sandbox:
            env = self.environment(record)
            await env.reconnect(sandbox)
            await env.stop(delete=True)
        for _ in range(15):
            if not await self.lookup(record["id"]):
                return
            await asyncio.sleep(1)
        raise RuntimeError("Daytona deletion remains unconfirmed")

    async def delete_orphan(self, allocation):
        sandbox = await self.lookup(allocation["id"])
        if sandbox:
            if sandbox.id != allocation["provider_id"]:
                raise RuntimeError("Daytona orphan identity changed")
            await (await self.client()).delete(sandbox, timeout=30, wait=True)
        if await self.lookup(allocation["id"]):
            raise RuntimeError("Daytona orphan deletion remains unconfirmed")
