"""Lifecycle recovery tests at the host and provider boundaries."""

import tempfile
import unittest
import uuid
from unittest.mock import AsyncMock

from lifecycle import HostError, Lifecycle


class LifecycleTests(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.assignment = dict(
            owner_id=str(uuid.uuid4()),
            epoch=1,
            expires_at="2099-01-01T00:00:00Z",
            job=dict(
                id=str(uuid.uuid4()),
                session_id="session",
                deadline="2099-01-01T00:00:00Z",
                operation_id="op",
                cancel_requested=False,
                spec={
                    "brief": {
                        "scope": {},
                        "retention_policy": "test",
                        "inputs": {"artifacts": []},
                    }
                },
            ),
        )
        self.effect = dict(id=str(uuid.uuid4()), state="prepared")

        async def command(**data):
            if data["action"] == "reserve":
                return {"reservation": {"id": "reservation"}}
            if data["action"] == "inspect_assignment":
                return {"assignment": self.assignment}
            if data["action"] == "prepare_effect":
                return {"effect": self.effect.copy()}
            if data["action"] == "report_effect":
                self.effect["state"] = data["state"]
            if data["action"] == "begin_effect":
                self.effect["state"] = "in_progress"
            return {"effect": self.effect.copy()}

        self.host = AsyncMock()
        self.host.command.side_effect = command
        self.provider = AsyncMock()
        self.provider.reservation_cost_microunits = 0
        self.provider.reservation_time_ms = 0
        self.provider.lookup.return_value = None
        self.provider.allocate.return_value = {"expires_at": 4070908800000}
        self.provider.allocations.return_value = []
        self.owner = Lifecycle(self.temp.name, self.host, self.provider)
        self.id = str(uuid.uuid4())

    def tearDown(self):
        self.owner.close()
        self.temp.cleanup()

    async def test_reopens_allocation_after_lost_ack_without_recreating_it(self):
        await self.owner.create(self.id, self.assignment, [])
        self.owner.close()
        self.owner = Lifecycle(self.temp.name, self.host, self.provider)
        self.provider.lookup.return_value = {"running": True}
        result = await self.owner.create(self.id, self.assignment, [])
        self.assertEqual(result, self.provider.allocate.return_value)
        begins = [
            call
            for call in self.host.command.call_args_list
            if call.kwargs["action"] == "begin_effect"
        ]
        self.assertEqual(len(begins), 1)

    async def test_cleanup_failure_remains_pending_across_restart(self):
        await self.owner.create(self.id, self.assignment, [])
        self.provider.delete.side_effect = RuntimeError("provider unavailable")
        with self.assertRaises(RuntimeError):
            await self.owner.finish(self.id)
        self.assertEqual(self.owner.get(self.id)["state"], "closing")
        self.owner.close()
        self.owner = Lifecycle(self.temp.name, self.host, self.provider)
        self.provider.delete.side_effect = None
        await self.owner.reconcile()
        self.assertEqual(self.owner.get(self.id)["state"], "deleted")

    async def test_publication_failure_prevents_teardown_and_reuses_download(self):
        export = {
            "id": str(uuid.uuid4()),
            "path": "/workspace/result",
            "label": "result",
            "max_bytes": 1024,
        }
        await self.owner.create(self.id, self.assignment, [export])
        self.provider.lookup.return_value = {"running": True}

        async def download(record, path, destination):
            destination.write_bytes(b"result")

        self.provider.export.side_effect = download
        self.host.publish.side_effect = RuntimeError("lost publication reply")
        with self.assertRaises(RuntimeError):
            await self.owner.finish(self.id, allow_lost_exports=True)
        self.provider.delete.assert_not_awaited()
        self.host.publish.side_effect = None
        self.host.publish.return_value = {"id": export["id"], "state": "ready"}
        result = await self.owner.finish(self.id)
        self.assertEqual(result["state"], "deleted")
        self.provider.export.assert_awaited_once()

    async def test_missing_output_after_cancellation_does_not_leak_sandbox(self):
        export = dict(
            id=str(uuid.uuid4()),
            path="/workspace/missing",
            label="result",
            max_bytes=100,
        )
        await self.owner.create(self.id, self.assignment, [export])
        self.provider.lookup.return_value = {"running": True}
        self.provider.export.side_effect = FileNotFoundError("not produced")
        result = await self.owner.finish(self.id, allow_lost_exports=True)
        self.assertEqual(result["artifacts"][0]["state"], "unavailable")
        self.provider.delete.assert_awaited_once()
        self.host.publish.assert_not_awaited()

    async def test_forbidden_host_access_is_not_evidence_of_expired_ownership(self):
        await self.owner.create(self.id, self.assignment, [])
        self.host.command.side_effect = HostError(403)
        result = await self.owner.reconcile()
        self.assertEqual(result[0]["state"], "unknown")
        self.provider.delete.assert_not_awaited()
        self.provider.fence.assert_not_awaited()

    async def test_owner_lock_excludes_a_second_bridge(self):
        with self.assertRaisesRegex(RuntimeError, "already has an owner"):
            Lifecycle(self.temp.name, self.host, self.provider)

    async def test_expired_ownership_stops_work_and_records_lost_exports(self):
        export = {
            "id": str(uuid.uuid4()),
            "path": "/workspace/result",
            "label": "result",
            "max_bytes": 1024,
        }
        await self.owner.create(self.id, self.assignment, [export])
        self.host.command.side_effect = HostError(409)
        result = await self.owner.reconcile()
        self.assertEqual(result[0]["state"], "deleted")
        self.assertEqual(result[0]["artifacts"][0]["state"], "unavailable")

    async def test_missing_ready_sandbox_does_not_silently_restart(self):
        await self.owner.create(self.id, self.assignment, [])
        with self.assertRaisesRegex(RuntimeError, "missing"):
            await self.owner.create(self.id, self.assignment, [])
        self.assertEqual(self.owner.get(self.id)["state"], "lost")


if __name__ == "__main__":
    unittest.main()
