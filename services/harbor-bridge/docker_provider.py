"""Docker allocation lookup around the pinned Harbor public lifecycle API."""

import asyncio
import json
import uuid
from pathlib import Path

from bridge import AllocationRequest, HarborAllocation
from control import export_file, fence_processes, renew_binding

LABEL = "io.agent-memory.allocation"
OWNER = "io.agent-memory.bridge"


class DockerProvider:
    reservation_time_ms = 0
    reservation_cost_microunits = 0

    def __init__(self, directory: Path, owner: str, image="memory-asp:wp05"):
        self.directory = directory.resolve()
        self.directory.mkdir(parents=True, exist_ok=True, mode=0o700)
        self.owner, self.image = str(uuid.UUID(owner)), image

    async def close(self):
        pass

    async def docker(self, *args):
        process = await asyncio.create_subprocess_exec(
            "docker",
            *args,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
        try:
            output, error = await asyncio.wait_for(process.communicate(), 60)
        except BaseException:
            process.kill()
            await process.wait()
            raise
        if process.returncode:
            raise RuntimeError("Docker operation failed: " + error.decode()[-2000:])
        return output.decode()

    async def containers(self):
        identifiers = (
            await self.docker("ps", "-aq", "--filter", f"label={OWNER}={self.owner}")
        ).split()
        if not identifiers:
            return []
        return json.loads(await self.docker("inspect", *identifiers))

    async def allocations(self):
        return [
            {
                "id": item["Config"]["Labels"][LABEL],
                "provider_id": item["Id"],
                "fence": {
                    "job_id": item["Config"]["Labels"]["io.agent-memory.job"],
                    "owner_id": item["Config"]["Labels"]["io.agent-memory.owner"],
                    "epoch": int(item["Config"]["Labels"]["io.agent-memory.epoch"]),
                },
            }
            for item in await self.containers()
        ]

    async def lookup(self, allocation_id):
        matches = [
            item
            for item in await self.containers()
            if item["Config"]["Labels"].get(LABEL) == allocation_id
        ]
        if len(matches) > 1:
            raise RuntimeError(
                "Allocation has multiple containers; reconciliation required"
            )
        return matches[0] if matches else None

    def environment(self, allocation_id, cpu, memory_mb, assignment=None):
        from harbor.environments.docker.docker import DockerEnvironment
        from harbor.models.task.config import EnvironmentConfig
        from harbor.models.trial.config import ResourceMode
        from harbor.models.trial.paths import TrialPaths

        allocation_id = str(uuid.UUID(allocation_id))
        if not 1 <= cpu <= 4 or not 128 <= memory_mb <= 4096:
            raise ValueError("Local profile allows 1–4 CPUs and 128–4096 MiB")
        root = self.directory / allocation_id
        environment = root / "environment"
        environment.mkdir(parents=True, exist_ok=True)
        # The image blocks new outbound traffic; SSH replies remain allowed.
        # Only private trial logs are mounted, never the project or .env.
        compose = {
            "services": {
                "main": {
                    "image": self.image,
                    "ports": ["127.0.0.1::22"],
                    "labels": {LABEL: allocation_id, OWNER: self.owner},
                    "networks": ["sandbox"],
                    "pids_limit": 128,
                    "cap_add": ["NET_ADMIN"],
                    "security_opt": ["no-new-privileges:true"],
                }
            },
            "networks": {
                "sandbox": {"labels": {OWNER: self.owner, LABEL: allocation_id}}
            },
        }
        if assignment:
            compose["services"]["main"]["labels"].update(
                {
                    "io.agent-memory.job": assignment["job"]["id"],
                    "io.agent-memory.owner": assignment["owner_id"],
                    "io.agent-memory.epoch": str(assignment["epoch"]),
                }
            )
        elif (environment / "docker-compose.yaml").exists():
            # Recover the declared ownership labels rather than overwriting them.
            prior = json.loads((environment / "docker-compose.yaml").read_text())
            compose["services"]["main"]["labels"] = prior["services"]["main"]["labels"]
        (environment / "docker-compose.yaml").write_text(json.dumps(compose))
        return DockerEnvironment(
            environment_dir=environment,
            environment_name="memory-asp",
            session_id=f"memory-{allocation_id}",
            trial_paths=TrialPaths(root / "trial"),
            task_env_config=EnvironmentConfig(
                docker_image=self.image, cpus=cpu, memory_mb=memory_mb
            ),
            cpu_enforcement_policy=ResourceMode.LIMIT,
            memory_enforcement_policy=ResourceMode.LIMIT,
        )

    async def allocate(self, record):
        request = record["request"]
        identifier = record["id"]
        prior = await self.lookup(identifier)
        env = self.environment(
            identifier, request["cpu"], request["memory_mb"], request["assignment"]
        )
        root = self.directory / identifier
        key = root / "identity"
        if not key.exists():
            if prior:
                raise RuntimeError(
                    "Allocation identity is missing; do not replace a live sandbox"
                )
            process = await asyncio.create_subprocess_exec(
                "ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(key)
            )
            if await process.wait():
                raise RuntimeError("Cannot create sandbox identity")
        assignment = request["assignment"]
        owner = AllocationRequest(
            identifier,
            identifier,
            assignment["job"]["session_id"],
            assignment["epoch"],
            request["expires_at"],
            "/workspace",
            "127.0.0.1",
            0,
            str(key),
            1000,
            1000,
        )
        allocation = HarborAllocation(env, owner)
        if prior:
            if not prior["State"]["Running"]:
                raise RuntimeError(
                    "Existing sandbox is stopped; reconcile rather than restart execution"
                )
            result = await env.exec(
                "cat /run/memory-asp/binding.json", user="root", timeout_sec=10
            )
            if result.return_code:
                raise RuntimeError(
                    "Allocation started without a usable binding; cleanup is required"
                )
            binding = json.loads(result.stdout)
            if any(
                binding[k] != getattr(owner, k)
                for k in ("sandbox_id", "generation", "session_id", "epoch")
            ):
                raise RuntimeError("Existing sandbox belongs to another assignment")
            await self.renew(record, request["expires_at"])
            descriptor = await allocation.inspect_binding()
        else:
            descriptor = await allocation.start()
            public = key.with_suffix(".pub").read_text().strip()
            authorized = root / "authorized_keys"
            authorized.write_text(
                'restrict,command="/usr/local/bin/python3 /opt/memory/asp_helper.py" '
                + public
                + "\n"
            )
            await env.upload_file(authorized, "/root/.ssh/authorized_keys")
            secured = await env.exec(
                "chown root:root /root/.ssh/authorized_keys && chmod 600 /root/.ssh/authorized_keys",
                user="root",
                timeout_sec=10,
            )
            if secured.return_code:
                raise RuntimeError("Cannot secure sandbox SSH key")
        actual = await self.lookup(identifier)
        if not actual or not actual["State"]["Running"]:
            raise RuntimeError("Sandbox disappeared during provisioning")
        ports = actual["NetworkSettings"]["Ports"].get("22/tcp") or []
        if len(ports) != 1 or ports[0]["HostIp"] != "127.0.0.1":
            raise RuntimeError(
                f"SSH port is not loopback restricted: ports={ports}, configured={actual['HostConfig']['PortBindings']}"
            )
        descriptor["descriptor"]["connection"]["port"] = int(ports[0]["HostPort"])
        await self.verify_limits(actual, request)
        return descriptor

    async def verify_limits(self, container, request):
        config = container["HostConfig"]
        if (
            config["NanoCpus"] != request["cpu"] * 1_000_000_000
            or config["Memory"] != request["memory_mb"] * 1024 * 1024
            or config["PidsLimit"] != 128
        ):
            raise RuntimeError("Provider resource limits do not match the profile")
        networks = list(container["NetworkSettings"]["Networks"])
        if len(networks) != 1:
            raise RuntimeError("Unexpected sandbox network")
        network = json.loads(await self.docker("network", "inspect", networks[0]))[0]
        if network["Labels"].get(OWNER) != self.owner:
            raise RuntimeError("Sandbox network has a different owner")
        env = self.environment(
            container["Config"]["Labels"][LABEL], request["cpu"], request["memory_mb"]
        )
        rules = await env.exec(
            "iptables -C OUTPUT -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT && iptables -C OUTPUT -j REJECT && ip6tables -C OUTPUT -j REJECT",
            user="root",
            timeout_sec=10,
        )
        if rules.return_code:
            raise RuntimeError("Sandbox outbound firewall is missing")

    def record_environment(self, record):
        return self.environment(
            record["id"], record["request"]["cpu"], record["request"]["memory_mb"]
        )

    async def renew(self, record, expires_at):
        await renew_binding(self.record_environment(record), record, expires_at)

    async def fence(self, record):
        if await self.lookup(record["id"]):
            await fence_processes(self.record_environment(record), record)

    async def export(self, record, path, destination):
        await export_file(self.record_environment(record), path, destination)

    async def delete(self, record):
        env = self.environment(
            record["id"], record["request"]["cpu"], record["request"]["memory_mb"]
        )
        await env.stop(delete=True)
        if await self.lookup(record["id"]):
            raise RuntimeError("Harbor returned before confirmed container deletion")
        # Harbor removes task networks; verify the task-labelled network as well.
        networks = (
            await self.docker(
                "network",
                "ls",
                "-q",
                "--filter",
                f"label={OWNER}={self.owner}",
                "--filter",
                f"label={LABEL}={record['id']}",
            )
        ).split()
        if networks:
            raise RuntimeError("Allocation network cleanup remains pending")

    async def delete_orphan(self, allocation):
        container = await self.lookup(allocation["id"])
        if not container:
            return
        if container["Id"] != allocation["provider_id"]:
            raise RuntimeError("Orphan identity changed")
        await self.docker("rm", "-f", container["Id"])
        if await self.lookup(allocation["id"]):
            raise RuntimeError("Orphan deletion was not confirmed")
        networks = (
            await self.docker(
                "network",
                "ls",
                "-q",
                "--filter",
                f"label={OWNER}={self.owner}",
                "--filter",
                f"label={LABEL}={allocation['id']}",
            )
        ).split()
        for network in networks:
            await self.docker("network", "rm", network)
