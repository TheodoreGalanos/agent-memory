# ABOUTME: Builds the production image, brings up deploy/compose.yaml with a throwaway instance
# ABOUTME: configuration, checks the health endpoints through the proxy, then tears everything down.
"""Smoke test for the production deployment profile (deploy/).

Exits non-zero and names the failing step on any error. Submits nothing paid: the generated
host.json has no worker_pool, so the Host never starts a pool child and never needs a provider
credential. If Docker is unavailable, this validates what it can without a daemon (compose config
syntax, and that every Dockerfile COPY source exists) and says so plainly instead of pretending to
have run the stack.
"""

import http.client
import json
import secrets
import shutil
import socket
import ssl
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DEPLOY = ROOT / "deploy"
COMPOSE_FILE = DEPLOY / "compose.yaml"
DOCKERFILE = DEPLOY / "Dockerfile"
RUNTIME = DEPLOY / ".runtime"  # gitignored by the repository's ".runtime/" pattern
PROJECT = f"memory-smoke-{secrets.token_hex(4)}"
READY_TIMEOUT_SECONDS = 180
# Matches the hostname baked into deploy/Caddyfile; Caddy needs a real site address (not a bare
# ":443" catch-all) to statically issue an internal certificate instead of demanding on-demand TLS.
PROXY_HOSTNAME = "memory.example.internal"


def fail(step: str, detail: str = "") -> None:
    message = f"FAILED at step: {step}"
    if detail:
        message += f"\n{detail}"
    raise SystemExit(message)


def run(args, **kwargs):
    kwargs.setdefault("cwd", ROOT)
    check = kwargs.pop("check", True)
    return subprocess.run(args, check=check, **kwargs)


def find_compose_command():
    """Return the argv prefix for a working `docker compose`, trying the CLI plugin first."""
    for candidate in (["docker", "compose"], ["docker-compose"]):
        try:
            subprocess.run(
                [*candidate, "version"],
                cwd=ROOT,
                check=True,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            return candidate
        except (subprocess.CalledProcessError, FileNotFoundError):
            continue
    return None


def docker_daemon_reachable() -> bool:
    if shutil.which("docker") is None:
        return False
    try:
        subprocess.run(
            ["docker", "info"],
            cwd=ROOT,
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        return True
    except (subprocess.CalledProcessError, FileNotFoundError):
        return False


def lint_dockerfile_copy_sources() -> list[str]:
    """Every literal COPY/ADD source in the Dockerfile must exist in the repository."""
    problems = []
    for line in DOCKERFILE.read_text().splitlines():
        words = line.split()
        if not words or words[0] not in ("COPY", "ADD"):
            continue
        if any(word.startswith("--from=") for word in words):
            continue  # Multi-stage copy; the source is another build stage, not a repository path.
        sources = [w for w in words[1:] if not w.startswith("--")]
        if len(sources) < 2:
            continue  # Not a recognizable "COPY src... dst" line; skip rather than guess.
        *sources, _destination = sources
        for source in sources:
            if not (ROOT / source).exists():
                problems.append(f"{DOCKERFILE.relative_to(ROOT)}: COPY source missing: {source}")
    return problems


def static_validation(compose_cmd) -> int:
    print("Docker is unavailable; validating what is possible without a daemon.")
    problems = lint_dockerfile_copy_sources()
    for problem in problems:
        print(f"Dockerfile lint: {problem}")
    if compose_cmd is not None:
        try:
            run([*compose_cmd, "-f", str(COMPOSE_FILE), "config"], stdout=subprocess.DEVNULL)
            print("docker compose config: syntax OK")
        except subprocess.CalledProcessError as error:
            print(f"docker compose config failed: {error}")
            problems.append("docker compose config")
    else:
        print("Neither `docker compose` nor `docker-compose` is installed; skipped config check.")
    if problems:
        fail("static validation", "\n".join(problems))
    print("Static validation passed. No live build or bring-up was performed.")
    return 0


def write_instance_configuration(run_dir: Path) -> Path:
    """Use the CLI's own `init` to generate credentials, then repoint at the container's Postgres."""
    instance_dir = run_dir / "instance"  # `init` creates this directory itself; it must not exist yet.
    run(
        [
            "node",
            "--experimental-strip-types",
            "packages/cli/src/main.ts",
            "init",
            str(instance_dir),
            "--port",
            "7331",
        ]
    )
    host_path = instance_dir / "host.json"
    host = json.loads(host_path.read_text())
    domain_password = secrets.token_hex(24)
    host["database_url"] = f"postgresql://memory_domain:{domain_password}@postgres:5432/memory"
    host["artifact_root"] = "/var/lib/memory/artifacts"
    host_path.write_text(json.dumps(host, indent=2) + "\n")
    (instance_dir / "worker.json").write_text("{}\n")
    secrets_dir = run_dir / "secrets"
    secrets_dir.mkdir()
    (secrets_dir / "memory_domain_password").write_text(domain_password)
    (secrets_dir / "postgres_admin_password").write_text(secrets.token_hex(24))
    (secrets_dir / "memory_session_password").write_text(secrets.token_hex(24))
    # Unused: host.json above has no worker_pool, so the pool never starts and never reads these.
    (secrets_dir / "azure_openai_api_key").write_text("unused-in-smoke-test")
    (secrets_dir / "jev_api_key").write_text("unused-in-smoke-test")
    (run_dir / "Caddyfile").write_text(
        "# Self-signed local certificate; no public domain is available for this smoke test.\n"
        f"{PROXY_HOSTNAME} {{\n\ttls internal\n\treverse_proxy 127.0.0.1:7331\n}}\n"
    )
    return instance_dir


def write_compose_override(run_dir: Path, instance_dir: Path) -> Path:
    override = run_dir / "compose.override.yaml"
    secret_names = [
        "postgres_admin_password",
        "memory_domain_password",
        "memory_session_password",
        "azure_openai_api_key",
        "jev_api_key",
    ]
    secrets_block = "\n".join(
        f"  {name}:\n    file: {run_dir / 'secrets' / name}" for name in secret_names
    )
    override.write_text(
        "services:\n"
        "  host:\n"
        "    volumes:\n"
        f"      - {instance_dir / 'host.json'}:/run/memory/host.json:ro\n"
        f"      - {instance_dir / 'worker.json'}:/run/memory/worker.json:ro\n"
        "  proxy:\n"
        "    volumes:\n"
        f"      - {run_dir / 'Caddyfile'}:/etc/caddy/Caddyfile:ro\n"
        "secrets:\n" + secrets_block + "\n"
    )
    return override


def _proxy_request(path: str, context: ssl.SSLContext, timeout: float) -> tuple[int, str]:
    """GET `path` through the proxy container's published port 443, with the SNI/Host it expects."""
    raw_sock = socket.create_connection(("127.0.0.1", 443), timeout=timeout)
    with context.wrap_socket(raw_sock, server_hostname=PROXY_HOSTNAME) as sock:
        connection = http.client.HTTPSConnection(PROXY_HOSTNAME, timeout=timeout)
        connection.sock = sock
        connection.request("GET", path, headers={"Host": PROXY_HOSTNAME})
        response = connection.getresponse()
        body = response.read().decode()
        return response.status, body


def wait_for_ready(context: ssl.SSLContext) -> None:
    deadline = time.monotonic() + READY_TIMEOUT_SECONDS
    last_error = ""
    while time.monotonic() < deadline:
        try:
            status, body = _proxy_request("/readyz", context, timeout=5)
            if status == 200 and "ready" in body:
                return
            last_error = f"HTTP {status}: {body}"
        except (OSError, ssl.SSLError) as error:
            last_error = str(error)
        time.sleep(3)
    fail("waiting for /readyz through the proxy", last_error)


def check_metrics(context: ssl.SSLContext) -> None:
    try:
        status, body = _proxy_request("/metrics", context, timeout=10)
        if status != 200:
            fail("checking /metrics", f"HTTP {status}: {body}")
    except (OSError, ssl.SSLError) as error:
        fail("checking /metrics", str(error))


def main() -> None:
    compose_cmd = find_compose_command()
    if not docker_daemon_reachable():
        sys.exit(static_validation(compose_cmd))
    if compose_cmd is None:
        fail("locating docker compose", "Docker is available but neither `docker compose` nor `docker-compose` runs.")

    RUNTIME.mkdir(exist_ok=True)
    run_dir = RUNTIME / PROJECT
    run_dir.mkdir()
    compose_args = [*compose_cmd, "-f", str(COMPOSE_FILE), "-p", PROJECT]
    try:
        try:
            instance_dir = write_instance_configuration(run_dir)
        except subprocess.CalledProcessError as error:
            fail("generating the throwaway instance configuration (init)", str(error))
        override = write_compose_override(run_dir, instance_dir)
        compose_args = [*compose_args, "-f", str(override)]

        try:
            run([*compose_args, "build"])
        except subprocess.CalledProcessError as error:
            fail("docker compose build", str(error))
        try:
            run([*compose_args, "up", "-d"])
        except subprocess.CalledProcessError as error:
            fail("docker compose up", str(error))

        context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
        context.check_hostname = False
        context.verify_mode = ssl.CERT_NONE
        wait_for_ready(context)
        check_metrics(context)
        print("Smoke test passed: the stack came up and /readyz and /metrics answered through the proxy.")
    finally:
        run([*compose_args, "down", "--volumes", "--rmi", "local"], check=False)
        shutil.rmtree(run_dir, ignore_errors=True)


if __name__ == "__main__":
    main()
