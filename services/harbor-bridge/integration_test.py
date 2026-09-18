"""Real Rust host -> Harbor/Docker -> Pi/ASP -> artifact -> verified teardown."""

import asyncio
import json
import os
import shlex
import sys
import uuid
from pathlib import Path

from lifecycle import HostClient, Lifecycle
from provider import configured_provider


async def main():
    fixture_path = Path(sys.argv[1])
    fixture = json.loads(fixture_path.read_text())
    root = Path(fixture["directory"]) / "sandbox"
    root.mkdir(mode=0o700)
    bridge_id = str(uuid.uuid4())
    provider_name = os.environ.get("MEMORY_SANDBOX_PROVIDER", "docker")
    host = HostClient(fixture["url"], fixture["admin_token"])
    identifier = str(uuid.uuid4())
    artifact_id = str(uuid.uuid4())
    exports = [
        {
            "id": artifact_id,
            "path": "/workspace/result.txt",
            "label": "Sandbox result",
            "media_type": "text/plain",
            "max_bytes": 1024,
        }
    ]
    config = root / "bridge.json"
    config.write_text(
        json.dumps(
            dict(
                provider=provider_name,
                env_file=str(Path.cwd() / ".env"),
                gateway_key=os.environ.get("MEMORY_DAYTONA_GATEWAY_KEY"),
                directory=str(root),
                bridge_id=bridge_id,
                host_url=fixture["url"],
                host_token=fixture["admin_token"],
                docker_host=os.environ.get("DOCKER_HOST"),
                docker_config=os.environ.get("DOCKER_CONFIG"),
            )
        )
    )
    config.chmod(0o600)
    provider = configured_provider(json.loads(config.read_text()))
    lifecycle = Lifecycle(root / "state", host, provider)
    try:
        memory_mb = 1024 if provider_name == "daytona" else 256
        binding = await lifecycle.create(
            identifier, fixture["assignment"], exports, memory_mb=memory_mb
        )
        allocated = await provider.lookup(identifier)
        first = allocated.id if provider_name == "daytona" else allocated["Id"]
        environment = (
            (await provider.attached(lifecycle.get(identifier)))[0]
            if provider_name == "daytona"
            else provider.environment(identifier, 1, memory_mb)
        )
        escaped = await environment.exec(
            "python3 -c "
            + shlex.quote(
                "import subprocess; subprocess.Popen(['sleep','60'],start_new_session=True,stdin=subprocess.DEVNULL,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)"
            ),
            user=1000,
            timeout_sec=10,
        )
        assert escaped.return_code == 0
        await provider.renew(lifecycle.get(identifier), 0)
        await asyncio.sleep(1.5)
        remaining = await environment.exec("pgrep -u 1000", user="root", timeout_sec=10)
        assert (
            remaining.return_code == 1
        ), "Expired sandbox retained generated processes"
        binding = await lifecycle.renew(identifier)
        lifecycle.close()
        lifecycle = Lifecycle(root / "state", host, provider)
        assert (
            await lifecycle.create(
                identifier, fixture["assignment"], exports, memory_mb=memory_mb
            )
            == binding
        )
        recovered = await provider.lookup(identifier)
        assert (
            recovered.id if provider_name == "daytona" else recovered["Id"]
        ) == first
        fixture.update(
            binding=binding,
            bridge_config=str(config),
            allocation_id=identifier,
            artifact_id=artifact_id,
        )
        fixture_path.write_text(json.dumps(fixture))
        lifecycle.close()
        lifecycle = None
        process = await asyncio.create_subprocess_exec(
            "npx",
            "vitest",
            "run",
            "packages/pi-worker/test/sandbox.test.ts",
            env={
                **{k: v for k, v in os.environ.items() if k != "DAYTONA_API_KEY"},
                "MEMORY_SANDBOX_FIXTURE": str(fixture_path),
            },
        )
        if await process.wait():
            lifecycle = Lifecycle(root / "state", host, provider)
            failed = lifecycle.get(identifier)
            print(
                "Lifecycle failure:",
                {k: failed.get(k) for k in ("state", "error")},
                flush=True,
            )
            raise AssertionError("Pi sandbox integration failed")
        lifecycle = Lifecycle(root / "state", host, provider)
        record = lifecycle.get(identifier)
        assert record["state"] == "deleted"
        assert record["artifacts"][0]["state"] == "ready"
        assert await provider.lookup(identifier) is None
        data = await host.send(
            "GET",
            f"/v1/artifacts/{artifact_id}?offset=0&limit=16",
            None,
            "application/octet-stream",
        )
        assert data == b"after\nfrom bash\n", repr(data)
        print(
            "Verified restart, Pi native tools, artifact bytes and container deletion"
        )
    finally:
        if lifecycle is None:
            lifecycle = Lifecycle(root / "state", host, provider)
        try:
            await lifecycle.finish(identifier, allow_lost_exports=True)
        except Exception:
            # Test fixtures contain no user work. Do not leak a paid test sandbox
            # when deliberately exercising a broken control/export path.
            await provider.delete(lifecycle.get(identifier))
            raise
        finally:
            lifecycle.close()
            await provider.close()


if __name__ == "__main__":
    asyncio.run(main())
