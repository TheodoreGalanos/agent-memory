"""Durable local sandbox work. The coordinator remains the ownership authority."""

import asyncio
import fcntl
import json
import sqlite3
import time
import uuid
from datetime import datetime
from pathlib import Path
from urllib.error import HTTPError
from urllib.parse import urlsplit
from urllib.request import HTTPRedirectHandler, Request, build_opener


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise HostError(code)


class HostError(RuntimeError):
    def __init__(self, status):
        self.status = status
        super().__init__(f"Host request failed ({status})")


class HostClient:
    def __init__(self, url, token):
        parsed = urlsplit(url)
        if (
            parsed.username
            or parsed.password
            or (
                parsed.scheme != "https"
                and not (
                    parsed.scheme == "http"
                    and parsed.hostname in ("127.0.0.1", "localhost", "::1")
                )
            )
        ):
            raise ValueError("Host requires HTTPS or loopback")
        self.url, self.token = url.rstrip("/"), token

    async def send(self, method, path, data, content_type):
        def request():
            # The constructor admits only HTTPS or loopback HTTP; no other scheme reaches here.
            req = Request(  # noqa: S310
                self.url + path,
                data=data,
                method=method,
                headers={
                    "Authorization": "Bearer " + self.token,
                    "Content-Type": content_type,
                },
            )
            try:
                with build_opener(NoRedirect).open(req, timeout=30) as response:
                    body = response.read(2 * 1024 * 1024 + 1)
                    if len(body) > 2 * 1024 * 1024:
                        raise RuntimeError("Host response exceeds limit")
                    return body
            except HTTPError as error:
                raise HostError(error.code) from None

        return await asyncio.to_thread(request)

    async def command(self, **command):
        return json.loads(
            await self.send(
                "POST", "/v1/commands", json.dumps(command).encode(), "application/json"
            )
        )

    async def publish(self, identifier, spec, path):
        artifact = (
            await self.command(action="allocate_artifact", id=identifier, spec=spec)
        )["artifact"]
        # The bridge's OS lock excludes another local publisher. Recovery is only
        # used after a previous bridge process has stopped, never during its call.
        if artifact["state"] == "uploading":
            artifact = (
                await self.command(
                    action="recover_artifact_upload",
                    id=identifier,
                    revision=artifact["revision"],
                )
            )["artifact"]
        if artifact["state"] != "ready":
            artifact = json.loads(
                await self.send(
                    "PUT",
                    "/v1/artifacts/" + identifier,
                    path.read_bytes(),
                    "application/octet-stream",
                )
            )
        if artifact["state"] != "ready":
            raise RuntimeError("Artifact publication did not settle")
        return artifact


def milliseconds(value):
    return int(datetime.fromisoformat(value.replace("Z", "+00:00")).timestamp() * 1000)


def fence(assignment):
    return dict(
        job_id=assignment["job"]["id"],
        owner_id=assignment["owner_id"],
        epoch=assignment["epoch"],
    )


class Lifecycle:
    def __init__(self, directory, host, provider):
        self.directory = Path(directory).resolve()
        self.directory.mkdir(parents=True, exist_ok=True, mode=0o700)
        self.lock = (self.directory / "owner.lock").open("a")
        try:
            fcntl.flock(self.lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BaseException:
            self.lock.close()
            raise RuntimeError("Sandbox lifecycle already has an owner") from None
        self.db = sqlite3.connect(self.directory / "allocations.sqlite")
        self.db.execute(
            "CREATE TABLE IF NOT EXISTS allocations (id TEXT PRIMARY KEY, data TEXT NOT NULL)"
        )
        self.db.commit()
        self.host, self.provider = host, provider

    def close(self):
        self.db.close()
        self.lock.close()

    def get(self, identifier):
        row = self.db.execute(
            "SELECT data FROM allocations WHERE id=?", (str(uuid.UUID(identifier)),)
        ).fetchone()
        if not row:
            raise KeyError("Unknown sandbox allocation")
        return json.loads(row[0])

    def save(self, record):
        self.db.execute(
            "INSERT INTO allocations VALUES (?,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data",
            (record["id"], json.dumps(record)),
        )
        self.db.commit()

    async def current(self, record):
        assignment = (
            await self.host.command(
                action="inspect_assignment",
                fence=fence(record["request"]["assignment"]),
            )
        )["assignment"]
        if assignment["job"]["cancel_requested"]:
            raise HostError(409)
        return assignment

    async def create(self, identifier, assignment, exports, cpu=1, memory_mb=256):
        identifier = str(uuid.UUID(identifier))
        if len(exports) > 16:
            raise ValueError("At most 16 declared exports")
        for export in exports:
            uuid.UUID(export["id"])
            if not 1 <= export["max_bytes"] <= 16 * 1024 * 1024:
                raise ValueError("Each export needs a byte allowance up to 16 MiB")
            if (
                not export["path"].startswith("/workspace/")
                or ".." in Path(export["path"]).parts
            ):
                raise ValueError("Exports must be inside the sandbox workspace")
        arguments = dict(
            cpu=cpu,
            memory_mb=memory_mb,
            exports=exports,
            session_id=assignment["job"]["session_id"],
            fence=fence(assignment),
        )
        try:
            record = self.get(identifier)
            if record["arguments"] != arguments:
                raise ValueError("Allocation identity was reused for different work")
            if record["state"] == "deleted":
                raise RuntimeError("Allocation is terminal; it cannot be recreated")
        except KeyError:
            duration = max(
                self.provider.reservation_time_ms,
                milliseconds(assignment["job"]["deadline"]) - int(time.time() * 1000),
                0,
            )
            record = dict(
                id=identifier,
                arguments=arguments,
                request=dict(
                    assignment=assignment,
                    cpu=cpu,
                    memory_mb=memory_mb,
                    exports=exports,
                    expires_at=milliseconds(assignment["expires_at"]),
                ),
                state="creating",
                binding=None,
                effect_id=None,
                artifacts=[],
                error=None,
                reservation_id=None,
                maximum=dict(
                    tokens=0,
                    provider_calls=0,
                    cost_microunits=self.provider.reservation_cost_microunits,
                    output_bytes=sum(item["max_bytes"] for item in exports),
                    sandbox_time_ms=duration,
                    sandbox_cpu_ms=cpu * duration,
                ),
            )
            self.save(record)
        current = await self.current(record)
        record["request"]["expires_at"] = milliseconds(current["expires_at"])
        self.save(record)
        reservation = (
            await self.host.command(
                action="reserve",
                fence=fence(assignment),
                provider_attempt="sandbox:" + identifier,
                maximum=record["maximum"],
                final_result=False,
            )
        )["reservation"]
        record["reservation_id"] = reservation["id"]
        self.save(record)
        effect = (
            await self.host.command(
                action="prepare_effect",
                fence=fence(assignment),
                request=dict(
                    logical_operation_id=assignment["job"]["operation_id"]
                    + "/sandbox:"
                    + identifier,
                    invocation_id="sandbox:" + identifier,
                    kind="sandbox.allocate",
                    replay="reconcile",
                    arguments=arguments,
                ),
            )
        )["effect"]
        record["effect_id"] = effect["id"]
        self.save(record)
        existing = await self.provider.lookup(identifier)
        if effect["state"] == "succeeded" and not existing:
            record["state"], record["error"] = (
                "lost",
                "Previously allocated sandbox is missing",
            )
            self.save(record)
            raise RuntimeError(record["error"])
        if effect["state"] == "outcome_unknown" and not existing:
            effect = (
                await self.host.command(
                    action="reconcile_effect",
                    effect_id=effect["id"],
                    resolution="not_performed",
                    evidence={"provider_lookup": "absent", "allocation_id": identifier},
                )
            )["effect"]
        if effect["state"] == "prepared":
            await self.host.command(
                action="begin_effect", fence=fence(assignment), effect_id=effect["id"]
            )
        elif effect["state"] == "failed":
            raise RuntimeError("Allocation effect failed; a new request is required")
        try:
            binding = await self.provider.allocate(record)
            # Allocation can take time. Check ownership again before exposing it.
            await self.current(record)
            receipt = dict(allocation_id=identifier, binding=binding)
            if effect["state"] == "outcome_unknown":
                await self.host.command(
                    action="reconcile_effect",
                    effect_id=effect["id"],
                    resolution="succeeded",
                    evidence=receipt,
                )
            elif effect["state"] != "succeeded":
                await self.host.command(
                    action="report_effect",
                    fence=fence(assignment),
                    effect_id=effect["id"],
                    state="succeeded",
                    receipt=receipt,
                )
            record["state"], record["binding"], record["error"] = "ready", binding, None
            self.save(record)
            return binding
        except Exception as error:
            record["state"], record["error"] = "unknown", type(error).__name__
            self.save(record)
            try:
                await self.host.command(
                    action="report_effect",
                    fence=fence(assignment),
                    effect_id=effect["id"],
                    state="outcome_unknown",
                    receipt={
                        "allocation_id": identifier,
                        "reason": "Allocation requires provider lookup",
                    },
                )
            except HostError:
                pass
            raise

    async def renew(self, identifier):
        record = self.get(identifier)
        if record["state"] != "ready":
            raise RuntimeError("Sandbox is not available for renewal")
        current = await self.current(record)
        expires = milliseconds(current["expires_at"])
        await self.provider.renew(record, expires)
        record["binding"]["expires_at"] = expires
        record["request"]["expires_at"] = expires
        self.save(record)
        return record["binding"]

    async def finish(self, identifier, allow_lost_exports=False):
        record = self.get(identifier)
        if record["state"] == "deleted":
            return record
        record["state"] = "closing"
        record["allow_lost_exports"] = allow_lost_exports or record.get(
            "allow_lost_exports", False
        )
        allow_lost_exports = record["allow_lost_exports"]
        self.save(record)
        try:
            exists = await self.provider.lookup(identifier)
            if exists:
                await self.provider.fence(record)
            for export in record["request"]["exports"]:
                if any(item["id"] == export["id"] for item in record["artifacts"]):
                    continue
                output = self.directory / (export["id"] + ".output")
                # A completed download is retained across a lost publication reply.
                if not output.exists():
                    if not exists and allow_lost_exports:
                        record["artifacts"].append(
                            {
                                "id": export["id"],
                                "state": "unavailable",
                                "reason": "Sandbox was lost before export",
                            }
                        )
                        self.save(record)
                        continue
                    if not exists:
                        raise RuntimeError("Sandbox was lost before artifact export")
                    pending = output.with_suffix(".part")
                    try:
                        await self.provider.export(record, export["path"], pending)
                    except FileNotFoundError:
                        if not allow_lost_exports:
                            raise
                        record["artifacts"].append(
                            {
                                "id": export["id"],
                                "state": "unavailable",
                                "reason": "Declared output was not produced",
                            }
                        )
                        self.save(record)
                        continue
                    if pending.stat().st_size > export["max_bytes"]:
                        raise RuntimeError(
                            "Sandbox artifact exceeds its reserved byte allowance"
                        )
                    pending.replace(output)
                brief = record["request"]["assignment"]["job"]["spec"]["brief"]
                spec = dict(
                    label=export["label"],
                    scope=brief["scope"],
                    media_type=export.get("media_type", "application/octet-stream"),
                    expected_bytes=output.stat().st_size,
                    origin="agent_generated",
                    retention_class=brief["retention_policy"],
                    dependencies=brief["inputs"]["artifacts"],
                )
                # A host outage is never evidence that the output can be discarded.
                artifact = await self.host.publish(export["id"], spec, output)
                record["artifacts"].append(artifact)
                self.save(record)
                output.unlink(missing_ok=True)
            await self.provider.delete(record)
            if record.get("reservation_id"):
                try:
                    # Docker supplies hard resource ceilings, not verified billing.
                    # Retain the maximum charge until authoritative usage is available.
                    await self.host.command(
                        action="settle_usage",
                        fence=fence(record["request"]["assignment"]),
                        reservation_id=record["reservation_id"],
                        observed=None,
                    )
                except HostError as error:
                    if error.status not in (404, 409):
                        raise
                record["usage"] = "unknown"
            record["state"], record["error"] = "deleted", None
            self.save(record)
            return record
        except Exception as error:
            record["error"] = type(error).__name__
            self.save(record)
            raise

    async def reconcile(self):
        results = []
        records = [
            json.loads(row[0])
            for row in self.db.execute("SELECT data FROM allocations")
        ]
        by_id = {record["id"]: record for record in records}
        for record in records:
            if record["state"] == "deleted":
                continue
            try:
                if record["state"] == "lost":
                    results.append(
                        await self.finish(record["id"], allow_lost_exports=True)
                    )
                    continue
                if record["state"] == "closing":
                    results.append(await self.finish(record["id"]))
                    continue
                try:
                    await self.current(record)
                except HostError as error:
                    if error.status not in (404, 409):
                        raise
                    results.append(
                        await self.finish(record["id"], allow_lost_exports=True)
                    )
                    continue
                request = record["request"]
                await self.create(
                    record["id"],
                    request["assignment"],
                    request["exports"],
                    request["cpu"],
                    request["memory_mb"],
                )
                results.append(self.get(record["id"]))
            except Exception as error:
                results.append(
                    {
                        "id": record["id"],
                        "state": "unknown",
                        "error": type(error).__name__,
                    }
                )
        for allocation in await self.provider.allocations():
            if allocation["id"] not in by_id:
                try:
                    await self.host.command(
                        action="inspect_assignment", fence=allocation["fence"]
                    )
                except HostError as error:
                    if error.status not in (404, 409):
                        raise
                    await self.provider.delete_orphan(allocation)
                    results.append({"id": allocation["id"], "state": "orphan_deleted"})
        return results
