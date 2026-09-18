"""Contract tests at Harbor's provider boundary; these do not qualify a provider."""

import json
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import AsyncMock, Mock

from bridge import AllocationRequest, HarborAllocation


class BridgeTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        self.environment = SimpleNamespace(
            validate_network_policy_support=Mock(),
            start=AsyncMock(),
            stop=AsyncMock(),
            upload_file=AsyncMock(),
            download_file=AsyncMock(),
            exec=AsyncMock(
                side_effect=[
                    SimpleNamespace(return_code=0, stdout="Python 3"),
                    SimpleNamespace(return_code=0, stdout=""),
                    SimpleNamespace(return_code=0, stdout=""),
                    SimpleNamespace(return_code=0, stdout="ssh-ed25519 YWJj test"),
                ]
            ),
        )
        self.request = AllocationRequest(
            "sandbox",
            "generation",
            "session",
            2,
            9999999999999,
            "/workspace",
            "localhost",
            2222,
            "/private/key",
            1000,
            1000,
        )
        self.allocation = HarborAllocation(self.environment, self.request)

    async def test_binding_uses_the_observed_host_key_and_assigned_owner(self):
        uploads = {}

        async def upload(source, destination):
            uploads[destination] = Path(source).read_text()

        self.environment.upload_file.side_effect = upload
        binding = await self.allocation.start()
        self.assertEqual(
            binding["descriptor"]["connection"]["host_key"], "ssh-ed25519 YWJj"
        )
        self.assertEqual(binding["epoch"], 2)
        owner = json.loads(uploads["/run/memory-asp/binding.json"])
        self.assertEqual(owner["exec_uid"], 1000)
        self.assertNotIn("identity_file", owner)
        self.environment.start.assert_awaited_once_with(force_build=False)

    async def test_unsupported_network_policy_fails_before_allocation(self):
        self.environment.validate_network_policy_support.side_effect = RuntimeError(
            "unsupported policy"
        )
        with self.assertRaisesRegex(RuntimeError, "unsupported policy"):
            await self.allocation.start()
        self.environment.start.assert_not_awaited()

    async def test_partial_start_attempts_cleanup(self):
        self.environment.start.side_effect = RuntimeError("start interrupted")
        with self.assertRaisesRegex(RuntimeError, "start interrupted"):
            await self.allocation.start()
        self.environment.stop.assert_awaited_once_with(delete=True)

    async def test_failed_cleanup_remains_an_error(self):
        self.environment.exec.side_effect = RuntimeError("probe failed")
        self.environment.stop.side_effect = RuntimeError("deletion unresolved")
        with self.assertRaisesRegex(RuntimeError, "deletion unresolved"):
            await self.allocation.start()


if __name__ == "__main__":
    unittest.main()
