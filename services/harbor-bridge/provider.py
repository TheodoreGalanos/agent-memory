"""Construct the selected provider from private operator configuration."""

import os
from pathlib import Path

from docker_provider import DockerProvider


def configured_provider(config):
    directory = Path(config["directory"]) / "provider"
    if config.get("provider", "docker") == "docker":
        if config.get("docker_host"):
            os.environ["DOCKER_HOST"] = config["docker_host"]
        if config.get("docker_config"):
            os.environ["DOCKER_CONFIG"] = config["docker_config"]
        return DockerProvider(directory, config["bridge_id"])
    if config["provider"] == "daytona":
        from daytona_provider import DaytonaProvider
        from dotenv import dotenv_values

        values = dotenv_values(config["env_file"])
        key = values.get("DAYTONA_API_KEY")
        if not key:
            raise ValueError("DAYTONA_API_KEY is not configured")
        os.environ["DAYTONA_API_KEY"] = key
        return DaytonaProvider(
            directory, config["bridge_id"], config.get("gateway_key")
        )
    raise ValueError("Unsupported sandbox provider")
