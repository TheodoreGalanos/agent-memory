"""Run the explicit, paid Daytona qualification profile with an approved spend cap."""

import argparse
import json
import os
import subprocess
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--budget-usd", type=float, required=True)
parser.add_argument("--gateway-key-file", type=Path)
args = parser.parse_args()
if not 0 < args.budget_usd <= 5:
    parser.error("This experimental run supports an approved limit up to US$5")
root = Path(__file__).resolve().parent.parent
# Conservative test allowance, not a provider billing report: 1 CPU / 1 GiB,
# 3 GiB disk, 15-minute provider TTL. Published compute rates imply under $0.02
# per full TTL; allow $0.25 per attempt to include build time and round up.
ledger = root / ".runtime/daytona-test-spend.json"
ledger.parent.mkdir(parents=True, exist_ok=True)
prior = json.loads(ledger.read_text()) if ledger.exists() else {"attempts": 0}
allowance = (prior["attempts"] + 1) * 0.25
if allowance > args.budget_usd:
    raise SystemExit(
        "Test spending allowance exhausted; inspect Daytona billing before another run"
    )
env = {**os.environ, "MEMORY_SANDBOX_PROVIDER": "daytona"}
if args.gateway_key_file:
    lines = [
        line
        for line in args.gateway_key_file.read_text().splitlines()
        if line.strip() and not line.startswith("#")
    ]
    if len(lines) != 1:
        raise SystemExit("Provide one approved gateway key")
    parts = lines[0].split()
    if parts[0].startswith("ssh-"):
        env["MEMORY_DAYTONA_GATEWAY_KEY"] = " ".join(parts[:2])
    else:
        env["MEMORY_DAYTONA_GATEWAY_KEY"] = " ".join(parts[1:3])
prior["attempts"] += 1
prior["reserved_usd"] = allowance
ledger.write_text(json.dumps(prior))
subprocess.run(
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
    cwd=root,
    env=env,
    check=True,
)
