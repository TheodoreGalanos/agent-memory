"""Pinned Harbor lifecycle binding. Provider credentials stay in this process."""

import json
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from harbor.environments.base import BaseEnvironment


@dataclass(frozen=True)
class AllocationRequest:
    sandbox_id: str
    generation: str
    session_id: str
    epoch: int
    expires_at: int
    workspace: str
    host: str
    port: int
    identity_file: str
    exec_uid: int
    exec_gid: int


class HarborAllocation:
    """Own one real Harbor environment created from a trusted deployment profile.

    Docker compose supplies the SSH port mapping and allocation label. The image
    must restrict the transport key to asp_helper.py, disable forwarding, and run
    generated code as the supplied unprivileged uid. Daytona substitutes its
    provider gateway connection after installing the same protected binding.
    """

    def __init__(self, environment: "BaseEnvironment", request: AllocationRequest):
        if (
            request.exec_uid <= 0
            or request.exec_gid <= 0
            or not request.workspace.startswith("/")
            or request.epoch < 1
        ):
            raise ValueError(
                "Allocation needs an unprivileged execution identity and absolute workspace"
            )
        self.environment = environment
        self.request = request

    async def start(self):
        env, request = self.environment, self.request
        # Harbor raises for network policies the selected provider cannot enforce.
        env.validate_network_policy_support()
        try:
            await env.start(force_build=False)
            probe = await env.exec(
                "python3 --version && test -x /usr/sbin/sshd", timeout_sec=10
            )
            if probe.return_code != 0:
                raise RuntimeError("Sandbox lacks Python or OpenSSH")
            created = await env.exec(
                "install -d -m 700 /run/memory-asp && install -d -m 755 /opt/memory",
                timeout_sec=10,
                user="root",
            )
            if created.return_code != 0:
                raise RuntimeError("Cannot prepare trusted sandbox control directory")
            await env.upload_file(
                Path(__file__).with_name("asp_helper.py"), "/opt/memory/asp_helper.py"
            )
            await env.upload_file(
                Path(__file__).with_name("interpreter.py"), "/opt/memory/interpreter.py"
            )
            owner = {
                key: getattr(request, key)
                for key in (
                    "sandbox_id",
                    "generation",
                    "session_id",
                    "epoch",
                    "expires_at",
                    "workspace",
                    "exec_uid",
                    "exec_gid",
                )
            }
            with tempfile.TemporaryDirectory(prefix="memory-binding-") as directory:
                path = Path(directory) / "binding.json"
                path.write_text(json.dumps(owner))
                path.chmod(0o600)
                await env.upload_file(path, "/run/memory-asp/binding.json")
            secured = await env.exec(
                "chown root:root /opt/memory/asp_helper.py /opt/memory/interpreter.py /run/memory-asp/binding.json && chmod 600 /run/memory-asp/binding.json && chmod 644 /opt/memory/asp_helper.py /opt/memory/interpreter.py",
                user="root",
                timeout_sec=10,
            )
            if secured.return_code != 0:
                raise RuntimeError("Cannot secure the sandbox control files")
            return await self.inspect_binding()
        except BaseException:
            # Failure here does not mean deletion succeeded. Let cleanup failure
            # reach the caller so the coordinator retains an unresolved effect.
            await env.stop(delete=True)
            raise

    async def inspect_binding(self):
        env, request = self.environment, self.request
        owner = {
            key: getattr(request, key)
            for key in ("sandbox_id", "generation", "session_id", "epoch", "expires_at")
        }
        key = await env.exec(
            "cat /etc/ssh/ssh_host_ed25519_key.pub", timeout_sec=10, user="root"
        )
        if key.return_code != 0 or not key.stdout:
            raise RuntimeError("Sandbox has no observed SSH host key")
        parts = key.stdout.strip().split()
        if len(parts) < 2 or parts[0] != "ssh-ed25519":
            raise RuntimeError("Sandbox returned an unsupported host key")
        return {
            **{
                key: owner[key]
                for key in (
                    "sandbox_id",
                    "generation",
                    "session_id",
                    "epoch",
                    "expires_at",
                )
            },
            "descriptor": {
                "version": "0",
                "transport": "ssh",
                "workspace": request.workspace,
                "connection": {
                    "host": request.host,
                    "port": request.port,
                    "user": "root",
                    "identity": {"file": request.identity_file},
                    "host_key": " ".join(parts[:2]),
                },
            },
        }
