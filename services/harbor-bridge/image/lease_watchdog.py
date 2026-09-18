"""Stop generated processes when endpoint authority expires, even if a group escaped."""

import json
import subprocess
import time
from pathlib import Path

while True:
    try:
        binding = json.loads(Path("/run/memory-asp/binding.json").read_text())
        active = time.time() * 1000 < binding["expires_at"]
    except (OSError, ValueError, KeyError):
        active = False
    if not active:
        # A missing process is not a failure; the watchdog only needs the kill attempted.
        subprocess.run(
            ["pkill", "-KILL", "-u", "1000"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )
    time.sleep(0.5)
