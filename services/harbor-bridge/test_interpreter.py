"""Real local interpreter process tests. Provider isolation is tested by ASP."""

import json
import os
import tempfile
import unittest
import uuid
from unittest.mock import patch

import interpreter


class InterpreterTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="repl-", dir="/tmp")
        self.workspace = self.temp.name
        self.session = str(uuid.uuid4())
        self.generation = None
        self.start()

    def tearDown(self):
        try:
            self.call("stop")
        except RuntimeError:
            pass
        self.temp.cleanup()

    def start(self):
        result = interpreter.request(
            self.workspace,
            {
                "action": "start",
                "session_id": self.session,
                "lifetime_seconds": 30,
                "idle_seconds": 20,
            },
        )
        self.assertEqual(result["status"], "ok")
        self.generation = result["generation"]

    def call(self, action, **values):
        return interpreter.request(
            self.workspace,
            {
                "action": action,
                "session_id": self.session,
                "generation": self.generation,
                **values,
            },
        )

    def execute(self, code, request_id=None, **values):
        return self.call(
            "execute", request_id=request_id or str(uuid.uuid4()), code=code, **values
        )

    def test_persistence_checkpoint_and_explicit_restore(self):
        request_id = str(uuid.uuid4())
        self.assertEqual(
            self.execute("items = [1, 2]; items.append(3)", request_id)["status"], "ok"
        )
        self.execute("items.append(4)")
        self.execute(
            "items = [99]", request_id
        )  # Duplicate receipt never executes again.
        checkpoint = self.call("checkpoint", names=["items"])
        self.assertEqual(checkpoint["objects"], {"items": [1, 2, 3, 4]})
        checkpoint = json.loads(json.dumps(checkpoint))
        old_generation = self.generation
        self.call("stop")
        self.start()
        self.assertNotEqual(self.generation, old_generation)
        self.assertEqual(self.execute("print(items)")["status"], "failed")
        self.assertEqual(
            self.call("restore", objects=checkpoint["objects"])["status"], "ok"
        )
        self.assertEqual(self.execute("print(sum(items))")["output"], "10\n")
        self.assertEqual(
            self.call("receipt", request_id=request_id)["generation"], old_generation
        )
        stale = interpreter.request(
            self.workspace,
            {
                "action": "checkpoint",
                "session_id": self.session,
                "generation": old_generation,
                "names": ["items"],
            },
        )
        self.assertIn("generation changed", stale["error"])

    def test_timeout_loses_heap_and_retains_uncertain_receipt(self):
        request_id = str(uuid.uuid4())
        result = self.execute(
            "import time; marker = 1; time.sleep(10)", request_id, timeout_seconds=1
        )
        self.assertEqual(result["status"], "unknown")
        self.assertIn("heap lost", result["error"])
        self.assertEqual(self.call("inspect")["status"], "lost")
        self.assertEqual(
            self.call("receipt", request_id=request_id)["status"], "unknown"
        )
        self.call("stop")
        self.start()
        replay = self.execute("print('must not run')", request_id)
        self.assertEqual(replay["status"], "unknown")
        self.assertNotIn("output", replay)

    def test_bounds_serialization_and_failure_effects(self):
        result = self.execute("state = 7; raise ValueError('after mutation')")
        self.assertEqual(result["status"], "failed")
        self.assertIn("may have changed", result["effects"])
        self.assertEqual(
            self.call("checkpoint", names=["state"])["objects"], {"state": 7}
        )
        self.assertEqual(self.execute("print('x' * 9000)")["status"], "failed")
        self.execute("function = lambda: 1")
        self.assertEqual(
            self.call("checkpoint", names=["function"])["status"], "failed"
        )
        self.assertEqual(
            self.call("checkpoint", names=["__builtins__"])["status"], "failed"
        )
        with self.assertRaisesRegex(ValueError, "64 KiB"):
            interpreter.encode({"value": "x" * 65536})

    def test_credentials_are_not_in_interpreter_environment(self):
        self.call("stop")
        with patch.dict(os.environ, {"MEMORY_TEST_PRIVATE_KEY": "test-only-value"}):
            self.start()
        self.assertEqual(
            self.execute("import os; print(os.getenv('MEMORY_TEST_PRIVATE_KEY'))")[
                "output"
            ],
            "None\n",
        )

    def test_abrupt_worker_exit_is_unknown_and_does_not_replay(self):
        request_id = str(uuid.uuid4())
        self.assertEqual(
            self.execute("import os; os._exit(1)", request_id)["status"], "unknown"
        )
        self.assertEqual(
            self.call("receipt", request_id=request_id)["status"], "unknown"
        )
