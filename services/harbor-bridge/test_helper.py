import base64
import json
import os
import signal
import subprocess
import tempfile
import time
import unittest
import uuid
from pathlib import Path

HELPER = Path(__file__).with_name("asp_helper.py")


class HelperTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="memory-helper-")
        self.root = Path(self.temp.name)
        self.workspace = self.root / "workspace"
        self.workspace.mkdir()
        self.control = self.root / "control"
        self.control.mkdir()
        self.binding = dict(
            sandbox_id="test",
            generation="one",
            session_id="session",
            epoch=1,
            expires_at=time.time() * 1000 + 30000,
            workspace=str(self.workspace),
            exec_uid=os.getuid(),
            exec_gid=os.getgid(),
        )
        (self.control / "binding.json").write_text(json.dumps(self.binding))
        self.executions = []

    def tearDown(self):
        for identifier in self.executions:
            try:
                self.rpc(op="cancel", execution_id=identifier)
            except Exception:
                pass
        for identifier in self.executions:
            path = self.control / "executions" / identifier / "status.json"
            if path.exists():
                status = json.loads(path.read_text())
                if status.get("state") == "running":
                    try:
                        os.killpg(status["pid"], signal.SIGKILL)
                    except ProcessLookupError:
                        pass
        self.temp.cleanup()

    def rpc(self, **request):
        response = subprocess.run(
            ["python3", str(HELPER)],
            input=json.dumps({**self.binding, **request}),
            capture_output=True,
            text=True,
            check=True,
            env={**os.environ, "MEMORY_ASP_CONTROL": str(self.control)},
            timeout=10,
        )
        return json.loads(response.stdout)

    def start(self, command, **options):
        identifier = str(uuid.uuid4())
        result = self.rpc(
            op="start",
            execution_id=identifier,
            command=command,
            timeout_ms=10000,
            max_output_bytes=1024 * 1024,
            **options,
        )
        self.assertTrue(result["ok"], result)
        self.executions.append(identifier)
        return identifier

    def finish(self, identifier):
        deadline = time.time() + 8
        while time.time() < deadline:
            response = self.rpc(op="status", execution_id=identifier)
            self.assertTrue(response["ok"], response)
            if response["value"]["state"] in ("finished", "outcome_unknown"):
                return response["value"]
            time.sleep(0.05)
        self.fail("Remote execution did not settle")

    def test_files_and_epoch(self):
        content = bytes(range(256))
        self.assertTrue(
            self.rpc(
                op="write",
                path="nested/space ' file.bin",
                data=base64.b64encode(content).decode(),
            )["ok"]
        )
        read = self.rpc(op="read", path="nested/space ' file.bin")
        self.assertEqual(base64.b64decode(read["value"]["data"]), content)
        self.assertTrue(
            self.rpc(
                op="rename", path="nested/space ' file.bin", destination="renamed.bin"
            )["ok"]
        )
        self.assertEqual(self.rpc(op="info", path="renamed.bin")["value"]["size"], 256)
        self.assertEqual(
            self.rpc(op="read", path="missing")["error"]["code"], "not_found"
        )
        self.assertEqual(
            self.rpc(op="read", path="renamed.bin", epoch=2)["error"]["code"],
            "permission_denied",
        )
        self.assertFalse(self.rpc(op="temp_file", prefix="../escape")["ok"])
        self.assertTrue(self.rpc(op="remove", path="nested", recursive=True)["ok"])

    def test_output_and_remote_cancellation(self):
        identifier = self.start("printf 'hello\\n'; printf 'error\\n' >&2; exit 7")
        result = self.finish(identifier)
        self.assertEqual(result["exit_code"], 7)
        self.assertTrue(result["process_group_gone"])
        output = self.rpc(op="output", execution_id=identifier)
        self.assertEqual(base64.b64decode(output["value"]), b"hello\nerror\n")
        identifier = self.start("sleep 30 & wait")
        time.sleep(0.1)
        self.rpc(op="cancel", execution_id=identifier)
        result = self.finish(identifier)
        self.assertEqual(result["reason"], "cancelled")
        self.assertTrue(result["process_group_gone"], result)

    def test_cancellation_after_output_is_closed(self):
        identifier = self.start("exec >/dev/null 2>&1; sleep 30")
        time.sleep(0.2)
        self.rpc(op="cancel", execution_id=identifier)
        result = self.finish(identifier)
        self.assertEqual(result["reason"], "cancelled")
        self.assertTrue(result["process_group_gone"], result)

    def test_output_limit_stops_the_producer(self):
        identifier = self.start("yes output")
        result = self.finish(identifier)
        self.assertEqual(result["reason"], "output_limit")
        self.assertLessEqual(Path(result["output_path"]).stat().st_size, 1024 * 1024)
        self.assertTrue(result["process_group_gone"], result)


if __name__ == "__main__":
    unittest.main()
