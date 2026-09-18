"""Run WP05 qualification on the configured local Docker engine."""

import json
import os
import subprocess
from pathlib import Path

root = Path(__file__).resolve().parent.parent
python = root / ".venv-harbor/bin/python"
if not python.exists():
    raise SystemExit(
        "Install services/harbor-bridge/requirements.txt in .venv-harbor first"
    )
env = os.environ.copy()
colima = Path.home() / ".colima/memory-wp05/docker.sock"
if "DOCKER_HOST" not in env and colima.exists():
    env["DOCKER_HOST"] = "unix://" + str(colima)
config = root / ".runtime/docker-config"
if "DOCKER_CONFIG" not in env and Path("/opt/homebrew/lib/docker/cli-plugins").exists():
    config.mkdir(parents=True, exist_ok=True)
    (config / "config.json").write_text(
        json.dumps({"cliPluginsExtraDirs": ["/opt/homebrew/lib/docker/cli-plugins"]})
    )
    env["DOCKER_CONFIG"] = str(config)
for args in [
    ["docker", "info", "--format", "{{.ServerVersion}}"],
    [
        "docker",
        "build",
        "-f",
        "services/harbor-bridge/image/Dockerfile",
        "-t",
        "memory-asp:wp05",
        "services/harbor-bridge",
    ],
    [
        "cargo",
        "test",
        "-p",
        "memory-host",
        "--test",
        "sandbox_lifecycle",
        "--",
        "--include-ignored",
    ],
]:
    subprocess.run(args, cwd=root, env=env, check=True)
