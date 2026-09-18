import { spawn } from "node:child_process";
import { once } from "node:events";
import { randomUUID } from "node:crypto";
import { mkdtemp, writeFile, rm } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { it, expect } from "vitest";
import { HostClient } from "../src/host-client.js";

it("serves authenticated typed commands through the Rust HTTP host", async () => {
  const root = await mkdtemp(join(tmpdir(), "memory-http-"));
  const token = randomUUID() + randomUUID();
  const workerToken = randomUUID() + randomUUID();
  const tenantId = randomUUID();
  const actorId = randomUUID();
  const config = join(root, "host.json");
  await writeFile(
    config,
    JSON.stringify({
      listen: "127.0.0.1:0",
      artifact_root: join(root, "artifacts"),
      database_url: `sqlite://${join(root, "host.sqlite")}?mode=rwc`,
      credentials: [
        {
          token: workerToken,
          tenant_id: tenantId,
          actor_id: actorId,
          scope: { entity_ids: [], source_versions: [] },
          role: { kind: "worker", job_id: randomUUID() },
          expires_at: new Date(Date.now() + 60_000).toISOString(),
        },
        {
          token,
          tenant_id: tenantId,
          actor_id: actorId,
          scope: { entity_ids: [], source_versions: [] },
          role: { kind: "administrator" },
          expires_at: new Date(Date.now() + 60_000).toISOString(),
        },
      ],
    }),
    { mode: 0o600 },
  );
  const child = spawn(
    join(process.cwd(), "target/debug/memory-host"),
    [config],
    { stdio: ["ignore", "pipe", "pipe"] },
  );
  let diagnostic = "";
  child.stderr.on("data", (data) => {
    diagnostic += data.toString();
  });
  try {
    const url = await new Promise<string>((resolve, reject) => {
      let text = "";
      child.stdout.on("data", (chunk) => {
        text += chunk.toString();
        const match = text.match(/listening on (127\.0\.0\.1:\d+)/);
        if (match) resolve(`http://${match[1]}`);
      });
      child.once("error", reject);
      child.once("exit", () =>
        reject(new Error(`Host exited before startup: ${diagnostic}`)),
      );
    });
    const api = new HostClient(url, token);
    const budget = {
      id: randomUUID(),
      scope: { entity_ids: [], source_versions: [] },
      deadline: new Date(Date.now() + 30_000).toISOString(),
      limit: {
        tokens: 100,
        provider_calls: 1,
        cost_microunits: 0,
        sandbox_cpu_ms: 0,
        sandbox_time_ms: 0,
        output_bytes: 0,
      },
      final_result_reserve: {
        tokens: 10,
        provider_calls: 0,
        cost_microunits: 0,
        sandbox_cpu_ms: 0,
        sandbox_time_ms: 0,
        output_bytes: 0,
      },
      max_child_depth: 1,
      max_child_concurrency: 1,
      pricing_revision: "test",
    };
    expect(
      await api.request({ action: "create_budget", budget }),
    ).toMatchObject({ kind: "budget", budget: { id: budget.id } });
    expect(
      await api.request({ action: "budget_usage", budget_id: budget.id }),
    ).toMatchObject({
      kind: "budget_usage",
      usage: { committed: { tokens: 0 } },
    });
    const artifactId = randomUUID();
    const artifact = await api.request({
      action: "allocate_artifact",
      id: artifactId,
      spec: {
        label: "Sandbox export",
        scope: { entity_ids: [], source_versions: [] },
        media_type: "application/octet-stream",
        expected_bytes: 4,
        origin: "agent_generated",
        retention_class: "test",
        dependencies: [],
      },
    });
    expect(artifact).toMatchObject({
      kind: "artifact",
      artifact: { state: "pending" },
    });
    const upload = await fetch(`${url}/v1/artifacts/${artifactId}`, {
      method: "PUT",
      headers: { authorization: `Bearer ${token}` },
      body: Buffer.from([0, 255, 1, 254]),
    });
    expect(upload.status).toBe(200);
    const download = await fetch(
      `${url}/v1/artifacts/${artifactId}?offset=0&limit=4`,
      { headers: { authorization: `Bearer ${token}` } },
    );
    expect(Buffer.from(await download.arrayBuffer())).toEqual(
      Buffer.from([0, 255, 1, 254]),
    );
    const unauthorizedUpload = await fetch(
      `${url}/v1/artifacts/${artifactId}`,
      { method: "PUT", body: Buffer.from([0, 255, 1, 254]) },
    );
    expect(unauthorizedUpload.status).toBe(401);
    const workerUpload = await fetch(`${url}/v1/artifacts/${artifactId}`, {
      method: "PUT",
      headers: { authorization: `Bearer ${workerToken}` },
      body: Buffer.from([0, 255, 1, 254]),
    });
    expect(workerUpload.status).toBe(403);
    const workerDownload = await fetch(
      `${url}/v1/artifacts/${artifactId}?offset=0&limit=4`,
      { headers: { authorization: `Bearer ${workerToken}` } },
    );
    expect(workerDownload.status).toBe(403);
    const duplicateUpload = await fetch(`${url}/v1/artifacts/${artifactId}`, {
      method: "PUT",
      headers: { authorization: `Bearer ${token}` },
      body: Buffer.from([0, 255, 1, 254]),
    });
    expect(duplicateUpload.status).toBe(409);
    const unauthorized = await fetch(`${url}/v1/commands`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ action: "recover" }),
    });
    expect(unauthorized.status).toBe(401);
    expect(await unauthorized.json()).toMatchObject({
      code: "unauthenticated",
    });
    const large = await fetch(`${url}/v1/commands`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        authorization: `Bearer ${token}`,
      },
      body: JSON.stringify({
        action: "recover",
        padding: "x".repeat(2 * 1024 * 1024),
      }),
    });
    expect(large.status).toBe(413);
    const openapi = (await (await fetch(`${url}/openapi.json`)).json()) as {
      openapi: string;
    };
    expect(openapi.openapi).toBe("3.1.0");
    expect((await fetch(`${url}/schemas/host-request.json`)).status).toBe(200);
    expect((await fetch(`${url}/schemas/missing.json`)).status).toBe(404);
  } finally {
    if (child.exitCode === null) {
      child.kill("SIGINT");
      await once(child, "exit");
    }
    await rm(root, { recursive: true, force: true });
  }
});
