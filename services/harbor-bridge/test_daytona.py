"""Pinned Harbor profile checks. These create no remote resources."""

import importlib.util
import tempfile
import time
import unittest
import uuid


@unittest.skipUnless(importlib.util.find_spec("daytona"), "requires .venv-harbor")
class DaytonaProfileTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        from daytona_provider import DaytonaProvider

        self.temp = tempfile.TemporaryDirectory()
        self.provider = DaytonaProvider(self.temp.name, str(uuid.uuid4()))
        self.record = {
            "id": str(uuid.uuid4()),
            "request": {
                "cpu": 1,
                "memory_mb": 1024,
                "expires_at": int((time.time() + 60) * 1000),
                "assignment": {
                    "epoch": 1,
                    "owner_id": str(uuid.uuid4()),
                    "job": {"id": str(uuid.uuid4())},
                },
            },
        }

    def tearDown(self):
        self.temp.cleanup()

    def test_profile_has_unique_name_bounded_ttl_and_no_ambient_secrets(self):
        from daytona import Image, Resources

        env = self.provider.environment(self.record)
        params = env._image_sandbox_params(
            image=Image.base("python:3.13-slim"),
            resources=Resources(cpu=1, memory=1, disk=3),
            network={"network_block_all": True},
        )
        self.assertEqual(params.name, "memory-" + self.record["id"])
        self.assertEqual(params.ttl_minutes, 15)
        self.assertTrue(params.network_block_all)
        self.assertFalse(params.env_vars)

    def test_resource_profile_cannot_silently_round_memory_down(self):
        self.record["request"]["memory_mb"] = 256
        with self.assertRaisesRegex(ValueError, "1024 MiB"):
            self.provider.environment(self.record)

    async def test_host_key_is_required_before_contacting_provider(self):
        from unittest.mock import AsyncMock

        self.provider.host_key = AsyncMock(side_effect=RuntimeError("no trusted key"))
        self.provider.lookup = AsyncMock()
        with self.assertRaisesRegex(RuntimeError, "no trusted key"):
            await self.provider.allocate(self.record)
        self.provider.lookup.assert_not_awaited()

    async def test_deleted_sandbox_can_remain_in_provider_list(self):
        from types import SimpleNamespace
        from unittest.mock import AsyncMock

        from daytona.common.errors import DaytonaNotFoundError

        self.provider.sandboxes = AsyncMock(
            return_value=[
                SimpleNamespace(
                    id="provider-id",
                    labels={"io.agent-memory.allocation": self.record["id"]},
                )
            ]
        )
        client = AsyncMock()
        client.get.side_effect = DaytonaNotFoundError("deleted")
        self.provider.client = AsyncMock(return_value=client)
        self.assertIsNone(await self.provider.lookup(self.record["id"]))
