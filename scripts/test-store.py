"""Run repository tests against SQLite and a disposable PostgreSQL cluster."""

import os
import subprocess
import tempfile
from pathlib import Path
from urllib.parse import urlencode


def run(*args, **kwargs):
    return subprocess.run(args, check=True, **kwargs)


def main():
    # pg_config also locates versioned installations whose binaries are not on PATH.
    bindir = Path(subprocess.check_output(["pg_config", "--bindir"], text=True).strip())
    for name in ("initdb", "pg_ctl"):
        if not (bindir / name).is_file():
            raise SystemExit(
                f"PostgreSQL development binaries required: {bindir / name}"
            )
    with tempfile.TemporaryDirectory(prefix="memory-pg-", dir="/tmp") as directory:
        root = Path(directory)
        data, sockets = root / "data", root / "socket"
        sockets.mkdir(mode=0o700)
        run(
            str(bindir / "initdb"),
            "-D",
            str(data),
            "-U",
            "memory_test",
            "-A",
            "trust",
            "--no-locale",
            "-E",
            "UTF8",
            stdout=subprocess.DEVNULL,
        )
        started = False
        try:
            run(
                str(bindir / "pg_ctl"),
                "-D",
                str(data),
                "-l",
                str(root / "postgres.log"),
                "-o",
                f"-c listen_addresses='' -k {sockets} -p 55432",
                "-w",
                "start",
            )
            started = True
            environment = os.environ.copy()
            environment["MEMORY_TEST_POSTGRES_URL"] = (
                "postgresql://memory_test@localhost/postgres?"
                + urlencode({"host": str(sockets), "port": 55432})
            )
            run(
                "cargo",
                "test",
                "-p",
                "memory-store",
                "--",
                "--include-ignored",
                env=environment,
            )
            run(
                "cargo",
                "test",
                "-p",
                "memory-host",
                "--test",
                "judgement",
                "--test",
                "formation",
                "--test",
                "activation",
                "--test",
                "consolidation",
                "--test",
                "maintenance",
                env=environment,
            )
            run(
                "npx",
                "vitest",
                "run",
                "packages/pi-worker/test/postgres.test.ts",
                env=environment,
            )
        except subprocess.CalledProcessError:
            log = root / "postgres.log"
            if log.exists():
                print(log.read_text())
            raise
        finally:
            if started:
                run(str(bindir / "pg_ctl"), "-D", str(data), "-m", "fast", "-w", "stop")


if __name__ == "__main__":
    main()
