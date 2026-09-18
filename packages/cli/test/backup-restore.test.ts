// ABOUTME: Exercised restore: back up a live instance, change it afterwards, restore the backup into a
// ABOUTME: new instance with the current deletion registry, and verify what the restored Host serves.
import { execFile, spawn, type ChildProcess } from "node:child_process";
import { promisify } from "node:util";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { once } from "node:events";
import { createServer } from "node:net";
import { randomUUID } from "node:crypto";
import { expect, it } from "vitest";
const exec = promisify(execFile);

async function freePort(): Promise<number> {
  const socket = createServer(); socket.listen(0, "127.0.0.1"); await once(socket, "listening");
  const address = socket.address(); if (!address || typeof address === "string") throw new Error("No port");
  await new Promise<void>((done, fail) => socket.close((e) => (e ? fail(e) : done()))); return address.port;
}
async function startHost(config: string): Promise<ChildProcess> {
  const host = spawn(resolve("target/debug/memory-host"), [config], { stdio: ["ignore", "pipe", "pipe"] });
  let errors = ""; host.stderr!.on("data", (c) => (errors += c));
  await Promise.race([once(host.stdout!, "data"), once(host, "exit").then(() => { throw new Error(errors); })]);
  return host;
}
async function stopHost(host: ChildProcess) { if (host.exitCode === null) { host.kill("SIGINT"); await once(host, "exit"); } }

it("restores a backup into an isolated instance and applies deletions recorded after the backup", async () => {
  const base = await mkdtemp(join(tmpdir(), "memory-restore-"));
  const original = join(base, "original"), restored = join(base, "restored"), backup = join(base, "backup");
  const cli = async (...args: string[]) => JSON.parse((await exec(process.execPath, ["--experimental-strip-types", "packages/cli/src/main.ts", ...args], { maxBuffer: 8 * 1024 * 1024 })).stdout);
  const port = await freePort();
  await cli("init", original, "--port", String(port));
  let host = await startHost(join(original, "host.json"));
  try {
    const step = async (name: string) => JSON.parse((await exec(process.execPath, ["examples/walkthrough.mjs", original, name], { maxBuffer: 8 * 1024 * 1024 })).stdout);
    await step("seed"); await step("correct"); await step("teach");
    const before = await cli("browse", "--config", join(original, "client.json"));
    expect(before.result.records).toHaveLength(2);
    const manifest = (await cli("backup", backup, "--config", join(original, "admin.json"))).manifest;
    expect(manifest.database.kind).toBe("sqlite");
    expect(manifest.artifacts.files).toBeGreaterThanOrEqual(0);
    // After the backup: one new record, and one existing record deleted (WP13 revocation).
    await step("explore"); await step("promote");
    const after = await cli("browse", "--config", join(original, "client.json"));
    expect(after.result.records).toHaveLength(3);
    const taught = after.result.records.find((r: { record: { label: string } }) => r.record.label === "How the property was found");
    const deletion = join(base, "deletion.json");
    await writeFile(deletion, JSON.stringify({ action: "begin_deletion", request: { id: randomUUID(), resources: [taught.reference.memory_id] } }));
    const report = await cli("call", "--config", join(original, "admin.json"), "--file", deletion);
    expect(report.kind).toBe("deletion");
    const registry = join(base, "registry.json");
    await writeFile(registry, JSON.stringify([report.report]));
    await stopHost(host);
    // Restore into a new directory on a new port with the current registry.
    const restorePort = await freePort();
    const result = await cli("restore", backup, restored, "--host", join(original, "host.json"), "--registry", registry, "--port", String(restorePort));
    expect(result.registry).toBe(registry);
    const restoredConfig = JSON.parse(await readFile(join(restored, "host.json"), "utf8"));
    expect(restoredConfig.restore_registry).toBe(registry);
    expect(restoredConfig.worker_pool).toBeUndefined();
    host = await startHost(join(restored, "host.json"));
    const served = await cli("browse", "--config", join(restored, "client.json"));
    const labels = served.result.records.map((r: { record: { label: string } }) => r.record.label).sort();
    // The promoted hypothesis postdates the backup; the taught episode was deleted after it.
    expect(labels).toEqual(["W-101 fire resistance"]);
    const history = await cli("history", "--config", join(restored, "client.json"), "--id", served.result.records[0].reference.memory_id);
    expect(history.result.records).toHaveLength(2);
  } finally {
    await stopHost(host);
    await rm(base, { recursive: true, force: true });
  }
}, 120_000);
