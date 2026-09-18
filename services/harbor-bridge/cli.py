"""One serialized lifecycle command; a supervisor can repeat reconcile after restart."""

import asyncio
import json
import sys
from pathlib import Path

from lifecycle import HostClient, Lifecycle
from provider import configured_provider


async def main():
    config_path = Path(sys.argv[1]).resolve()
    if config_path.stat().st_mode & 0o077:
        raise ValueError("Bridge configuration must be private (chmod 600)")
    config = json.loads(config_path.read_text())
    provider = configured_provider(config)
    lifecycle = Lifecycle(
        Path(config["directory"]) / "state",
        HostClient(config["host_url"], config["host_token"]),
        provider,
    )
    try:
        raw = sys.stdin.buffer.read(2 * 1024 * 1024 + 1)
        if len(raw) > 2 * 1024 * 1024:
            raise ValueError("Command too large")
        request = json.loads(raw)
        action = request.pop("action")
        if action not in ("create", "renew", "finish", "reconcile"):
            raise ValueError("Unsupported lifecycle command")
        result = await getattr(lifecycle, action)(**request)
        print(json.dumps(result))
    finally:
        lifecycle.close()
        await provider.close()


if __name__ == "__main__":
    try:
        asyncio.run(main())
    except Exception as error:
        # Never print configuration, credentials or an HTTP request object.
        print(
            json.dumps(
                {
                    "error": type(error).__name__,
                    "status": getattr(error, "status", None),
                    "message": "Lifecycle failed; inspect private allocation state",
                }
            ),
            file=sys.stderr,
        )
        sys.exit(1)
