// ABOUTME: Opt-in paid end-to-end run of the live walkthrough: model task, capture, formation with
// ABOUTME: Azure reference and Jev shadow, then inspection. Writes a transcript report for the guide.
import { execFile, spawn, type ChildProcess } from "node:child_process";
import { promisify } from "node:util";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { once } from "node:events";
import { createServer } from "node:net";
import { expect, it } from "vitest";

const exec = promisify(execFile);
const enabled = process.env.MEMORY_LIVE_WALKTHROUGH === "1";

async function freePort(): Promise<number> {
  const socket = createServer();
  socket.listen(0, "127.0.0.1");
  await once(socket, "listening");
  const address = socket.address();
  if (!address || typeof address === "string") throw new Error("No port");
  await new Promise<void>((done, fail) => socket.close((e) => (e ? fail(e) : done())));
  return address.port;
}
const hostLog: string[] = [];
async function startHost(config: string): Promise<ChildProcess> {
  const host = spawn(resolve("target/debug/memory-host"), [config], { stdio: ["ignore", "pipe", "pipe"] });
  let errors = "";
  host.stderr!.on("data", (chunk) => (errors += chunk));
  host.stdout!.on("data", (chunk) => hostLog.push(String(chunk)));
  await Promise.race([
    once(host.stdout!, "data"),
    once(host, "exit").then(() => { throw new Error(errors); }),
    new Promise((_, reject) => setTimeout(() => reject(new Error("Host startup timeout")), 10_000).unref()),
  ]);
  return host;
}
async function stopHost(host: ChildProcess) {
  if (host.exitCode === null) {
    host.kill("SIGINT");
    await once(host, "exit");
  }
}

it.skipIf(!enabled)("runs the live walkthrough: model task, capture, formation with Jev shadow, inspection", async () => {
  const reportPath = process.env.MEMORY_LIVE_WALKTHROUGH_REPORT;
  if (!reportPath) throw new Error("An explicit unused report path is required (MEMORY_LIVE_WALKTHROUGH_REPORT)");
  const report: { date: string; steps: { step: string; output: string }[]; host_log?: string; records?: unknown; manifests?: number } = { date: new Date().toISOString(), steps: [] };
  await writeFile(reportPath, JSON.stringify(report, null, 2) + "\n", { flag: "wx" });
  const save = () => writeFile(reportPath, JSON.stringify(report, null, 2) + "\n");

  const base = await mkdtemp(join(tmpdir(), "memory-live-walkthrough-")), directory = join(base, "demo");
  const port = await freePort();
  const cli = async (command: string, ...args: string[]) =>
    JSON.parse((await exec(process.execPath, ["--experimental-strip-types", "packages/cli/src/main.ts", command, ...args], { maxBuffer: 8 * 1024 * 1024 })).stdout);
  const base_step = async (name: string) =>
    JSON.parse((await exec(process.execPath, ["examples/walkthrough.mjs", directory, name], { maxBuffer: 8 * 1024 * 1024 })).stdout);
  const live = async (name: string) => {
    const { stdout } = await exec(process.execPath, ["--import", "./scripts/ts-loader.mjs", "examples/live-walkthrough.ts", directory, name], { maxBuffer: 8 * 1024 * 1024 });
    report.steps.push({ step: name, output: stdout });
    await save();
    return stdout;
  };
  await cli("init", directory, "--port", String(port));
  // Part 1 runs without a pool; enabling the pool is a configuration change, applied on restart.
  let host = await startHost(join(directory, "host.json"));
  try {
    await base_step("seed");
    await base_step("correct");
    await base_step("teach");
    await stopHost(host);
    await cli("enable-pool", directory);
    host = await startHost(join(directory, "host.json"));
    const prepared = await live("prepare");
    expect(prepared).toContain("queued");
    expect(prepared).not.toContain("Warning");

    const worked = await live("work");
    expect(worked).toMatch(/Result status (partial|complete); [1-9]\d* finding\(s\)/);
    expect(worked).toContain("render responded");

    const captured = await live("capture");
    expect(captured).toContain("Published source");
    expect(captured).toContain("Formation job");
    // A Host restart also restarts its pool; the queued formation job is picked up afterwards.
    await stopHost(host);
    host = await startHost(join(directory, "host.json"));

    const formed = await live("form");
    expect(formed).toMatch(/Retained [1-9]\d* candidate record\(s\)/);
    expect(formed).toContain("did not decide retention");

    const shown = await live("show");
    expect(shown).toMatch(/finding-1 r1/);
    const state = JSON.parse(await readFile(join(directory, "live", "state.json"), "utf8"));
    const task = await cli("task", "--config", join(directory, "client.json"), "--id", state.task.job_id);
    expect(task.result.manifests.length).toBeGreaterThan(0);
    expect(task.result.manifests[0].status).toBe("responded");
    const records = await cli("browse", "--config", join(directory, "client.json"));
    const derived = records.result.records.filter((r: { record: { label: string } }) => r.record.label.includes(": finding-"));
    expect(derived.length).toBeGreaterThan(0);
    for (const item of derived) expect(item.record.qualification.status).toBe("candidate");
    report.records = records.result.records;
    report.manifests = task.result.manifests.length;
    report.host_log = hostLog.join("");
    expect(report.host_log).toContain("shadow jev");
    expect(report.host_log).toContain("reference azure-reference");
    await save();
  } finally {
    await stopHost(host);
    await rm(base, { recursive: true, force: true });
  }
}, 900_000);
