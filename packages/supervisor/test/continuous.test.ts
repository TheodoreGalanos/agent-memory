// ABOUTME: A headless continuous scenario against the real production entrypoint (packages/supervisor/src/
// ABOUTME: main.ts, spawned as its own process with a worker.json), including a mid-scenario kill/restart.
import { spawn, type ChildProcess } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { once } from "node:events";
import { createServer } from "node:net";
import { randomBytes, randomUUID } from "node:crypto";
import { expect, it } from "vitest";
import type { HostRequest } from "../../../contracts/generated/host-request.js";
import type { HostResponse, Job } from "../../../contracts/generated/host-response.js";
import { HostClient } from "../../host-client/src/client.js";

async function freePort(): Promise<number> {
  const socket = createServer();
  socket.listen(0, "127.0.0.1");
  await once(socket, "listening");
  const address = socket.address();
  if (!address || typeof address === "string") throw new Error("No port");
  await new Promise<void>((done, fail) => socket.close((e) => (e ? fail(e) : done())));
  return address.port;
}
const minutes = (n: number) => new Date(Date.now() + n * 60_000).toISOString();
function expectKind<K extends HostResponse["kind"]>(response: HostResponse, kind: K): Extract<HostResponse, { kind: K }> {
  if (response.kind !== kind) throw new Error(`Expected ${kind}, got ${response.kind}: ${JSON.stringify(response).slice(0, 300)}`);
  return response as Extract<HostResponse, { kind: K }>;
}
const SETTLED = ["completed", "partial", "failed"];
// The scripted task model always answers fully, but the pool's own decision/coverage checks may
// still settle a job "partial" for reasons unrelated to what this scenario proves (scheduling,
// retrieval and recovery); either is an acceptable settled outcome here.
const SETTLED_OK = ["completed", "partial"];
async function waitFor(check: () => Promise<boolean>, description: string, timeoutMs = 30_000) {
  const started = Date.now();
  while (!(await check())) {
    if (Date.now() - started > timeoutMs) throw new Error(`Timed out waiting for ${description}`);
    await new Promise((r) => setTimeout(r, 300));
  }
}

/** A pool child of the real entrypoint, spawned as its own OS process against the given Host,
 * scripted judgement/task-model routes only (`MEMORY_TEST_SCRIPTED=1`) — no live model, no credentials. */
function spawnPool(hostUrl: string, token: string, workerConfigPath: string): ChildProcess {
  const child = spawn(
    process.execPath,
    ["--import", resolve("scripts/ts-loader.mjs"), resolve("packages/supervisor/src/main.ts"), workerConfigPath],
    { env: { ...process.env, MEMORY_HOST_URL: hostUrl, MEMORY_POOL_TOKEN: token, MEMORY_POOL_CLASSES: "interactive,deferred", MEMORY_TEST_SCRIPTED: "1" }, stdio: ["ignore", "pipe", "pipe"] },
  );
  return child;
}

it("forms memory from a source-backed task, a later task retrieves it, and the pool recovers after a mid-task kill and restart", async () => {
  const base = await mkdtemp(join(tmpdir(), "memory-continuous-"));
  const port = await freePort();
  const tenant_id = randomUUID(), actor_id = randomUUID(), project_id = randomUUID();
  const scope = { user_id: null, project_id, task_id: null, entity_ids: [], source_versions: [] };
  const tokens = { client: randomBytes(32).toString("hex"), admin: randomBytes(32).toString("hex"), pool: randomBytes(32).toString("hex") };
  const expires_at = minutes(60);
  await writeFile(join(base, "host.json"), JSON.stringify({
    listen: `127.0.0.1:${port}`, database_url: `sqlite://${join(base, "memory.db")}?mode=rwc`, artifact_root: join(base, "artifacts"),
    credentials: [
      { token: tokens.client, tenant_id, actor_id, scope, role: { kind: "client" }, expires_at },
      { token: tokens.admin, tenant_id, actor_id: randomUUID(), scope: { ...scope, project_id: null }, role: { kind: "administrator" }, expires_at },
      { token: tokens.pool, tenant_id, actor_id: randomUUID(), scope: { ...scope, project_id: null }, role: { kind: "pool", classes: ["interactive", "deferred"] }, expires_at },
    ],
  }), { mode: 0o600 });
  const host = spawn(resolve("target/debug/memory-host"), [join(base, "host.json")], { stdio: ["ignore", "pipe", "pipe"] });
  let hostErrors = ""; host.stderr!.on("data", (c) => (hostErrors += c));
  await Promise.race([once(host.stdout!, "data"), once(host, "exit").then(() => { throw new Error(hostErrors); })]);
  const url = `http://127.0.0.1:${port}`;
  const client = new HostClient(url, tokens.client), admin = new HostClient(url, tokens.admin);
  let poolLog = "";
  let pool: ChildProcess | undefined;

  try {
    const created = expectKind(await admin.request({ action: "user", request: { action: "mutate", request_id: randomUUID(), mutation: {
      action: "create_policy", label: "Continuous scenario", scope, effective: { kind: "unknown" },
      policy: { retention_purpose: "test", allowed_uses: ["task_context"], source_rules: [], evidence_requirements: [], applicability_rules: [], budget_class: "test", scheduling_priority: 0, judgement_dispositions: [], notification_policy: "quiet", qualification_requirements: [] } } } }), "user").result;
    if (created.kind !== "policy") throw new Error("policy");
    const budget = { id: randomUUID(), scope, limit: { tokens: 5_000_000, provider_calls: 200, output_bytes: 5_000_000, cost_microunits: 5_000_000, sandbox_cpu_ms: 0, sandbox_time_ms: 0 }, final_result_reserve: { tokens: 100, provider_calls: 0, output_bytes: 1000, cost_microunits: 0, sandbox_cpu_ms: 0, sandbox_time_ms: 0 }, deadline: minutes(60), max_child_depth: 1, max_child_concurrency: 2, pricing_revision: "test" };
    await admin.request({ action: "create_budget", budget });
    const profile = { id: randomUUID(), revision: 1, label: "continuous" };
    const cwd = await mkdtemp(join(tmpdir(), "memory-continuous-cwd-"));
    const phrase = "clearance under beam B3 measured at 2.0 m";
    await writeFile(join(cwd, "site-notes.txt"), `Site notes: ${phrase}`);
    const submit = async (purpose: string, extra: Record<string, unknown> = {}) => {
      const task_id = randomUUID();
      const brief = {
        schema_version: "1", purpose, process: "investigation", task_id, frame_ref: null, definitions: [],
        scope: { ...scope, task_id }, inputs: { memories: [], sources: [], artifacts: [] }, capabilities: { sources: [], queries: [], tools: [] },
        known_conflicts: [], evidence_cutoff: new Date().toISOString(), freshness_requirement: "current", output_criteria: ["Answered"],
        limits: { root_budget_id: budget.id, max_provider_attempts: 8, max_tokens: 4_000_000, max_output_bytes: 1_000_000, max_child_depth: 1, max_child_concurrency: 2 },
        policy: created.policy.reference, profile, retention_policy: "test", disclosure_policy: "test", ...extra,
      };
      // A generous attempt budget: a task-model call slow enough to make pi-ai yield and re-poll
      // (as task C's does, to leave a real window for a mid-flight kill) consumes an attempt each
      // time it is reclaimed, on top of the crash/restart cycle itself — not just crash recovery.
      const command = { request_id: randomUUID(), command_version: "1", tenant_id, actor_id, scope: brief.scope, job_id: null, session_id: null, operation_id: null, lane_id: null, invocation_id: null, expected_revisions: [], deadline: minutes(20), budget_id: budget.id, lease_epoch: null, payload: { brief, parent_id: null, max_attempts: 6, retain_until: minutes(1440) } };
      return expectKind(await client.request({ action: "submit", command } as HostRequest), "job").job;
    };
    const jobState = async (id: string): Promise<Job> => expectKind(await client.request({ action: "inspect_job", job_id: id }), "job").job;

    // The scripted judgement route answers formation's candidate families (J30's exchange/message
    // role classification is structural, handled by the route itself) and admission-time activation's
    // own retrieval families.
    const answersFile = join(base, "answers.json");
    await writeFile(answersFile, JSON.stringify({
      "J01.assessment": "observation",
      "J01.faithfulness": "yes", "J02.assessment": "supports", "J03.assessment": "boundary",
      "J04.assessment": "conditional_method", "J05.assessment": "commitment", "J27.assessment": "task_local",
      "J06.direct": "yes", "J06.contradiction": "no", "J06.background": "no", "J06.exception": "no", "J07.assessment": "relevant", "J08.assessment": "established",
    }));
    const repliesFile = join(base, "replies.json");
    await writeFile(repliesFile, JSON.stringify([
      { tool_call: { name: "read", arguments: { path: "site-notes.txt" } } },
      { status: "complete", coverage: { examined: ["Read site-notes.txt for the crane route clearance"], unexamined: [] } },
      { status: "complete", coverage: { examined: ["Answered"], unexamined: [] } },
      // Task C's own reply: held back long enough to observe the job "leased"/"running" before it
      // settles, so killing the pool child mid-task is deterministic rather than a timing race.
      { status: "complete", coverage: { examined: ["Answered"], unexamined: [] }, delay_ms: 5000 },
    ]));
    const workerConfigPath = join(base, "worker.json");
    await writeFile(workerConfigPath, JSON.stringify({
      sessions_directory: "sessions", processes: ["investigation", "formation", "activation"],
      task_model: { provider: "faux", replies_file: "replies.json" },
      judgement: { primary: { kind: "scripted", answers_file: "answers.json" } },
      execution: { kind: "node", cwd },
      // A short lease (with a fast poll) keeps the recovery window observable within a normal
      // test timeout instead of the production default (120s).
      poll_ms: 100, lease_seconds: 3, drain_ms: 500,
    }));

    pool = spawnPool(url, tokens.pool, workerConfigPath);
    pool.stdout!.on("data", (c) => (poolLog += c));
    pool.stderr!.on("data", (c) => (poolLog += c));
    await Promise.race([
      once(pool.stdout!, "data"),
      once(pool, "exit").then(([code]) => { throw new Error(`pool exited (${code}) before starting:\n${poolLog}`); }),
    ]);

    // Task A reads the file with the real `read` tool under a node execution service: its own
    // recorded experience (not any separately ingested source) is what the Host's own scheduler
    // forms memory from automatically once the task settles.
    const taskA = await submit("What clearance is available under beam B3 for the crane route?", { capabilities: { sources: [], queries: [], tools: ["read"] } });
    await waitFor(async () => SETTLED.includes((await jobState(taskA.id)).state), "task A to settle", 60_000);
    expect(SETTLED_OK.includes((await jobState(taskA.id)).state), poolLog).toBe(true);

    const scheduledFormation = async () => {
      const page = expectKind(await client.request({ action: "events", after: 0, limit: 500 }), "events").page;
      for (const event of page.events.filter((e) => e.kind === "job_accepted")) {
        const candidate = await jobState(event.resource_id);
        if (candidate.spec.brief.process === "formation") return candidate;
      }
      return undefined;
    };
    let formationJob: Job | undefined;
    await waitFor(async () => (formationJob = await scheduledFormation()) !== undefined, "the Host to schedule formation", 30_000);
    await waitFor(async () => SETTLED.includes((await jobState(formationJob!.id)).state), "formation to settle", 60_000);
    expect(SETTLED_OK.includes((await jobState(formationJob!.id)).state), poolLog).toBe(true);
    const browse = expectKind(await client.request({ action: "user", request: { action: "browse", query: { include_inactive: false, limit: 50 } } }), "user").result;
    if (browse.kind !== "records") throw new Error("records");
    expect(browse.records.length, poolLog).toBeGreaterThan(0);

    // Task B, in the same project, retrieves the formed memory at admission (the pool child's own
    // "activation:" log line is the durable, out-of-process proof; nothing here reruns its internals).
    const beforeTaskB = poolLog.length;
    const taskB = await submit("What clearance is available under beam B3 for the crane route?");
    await waitFor(async () => SETTLED.includes((await jobState(taskB.id)).state), "task B to settle", 60_000);
    expect(SETTLED_OK.includes((await jobState(taskB.id)).state), poolLog).toBe(true);
    const taskBLog = poolLog.slice(beforeTaskB);
    expect(taskBLog).toContain("activation:");
    expect(taskBLog.includes("activation: 0 memory"), taskBLog).toBe(false);

    // A third task is claimed, then the pool child is killed mid-task (no start, no result): the
    // Host's own recovery timer requeues the abandoned lease, and a freshly restarted pool child
    // completes it — proof that recovery works against the real entrypoint, not a test harness stub.
    const taskC = await submit("A third task, interrupted mid-flight.");
    await waitFor(async () => { const s = (await jobState(taskC.id)).state; return s === "leased" || s === "running"; }, "the pool to claim task C", 30_000);
    pool.kill("SIGKILL");
    await once(pool, "exit");
    await waitFor(async () => (await jobState(taskC.id)).state === "queued", "recovery to requeue task C", 30_000);
    poolLog = "";
    pool = spawnPool(url, tokens.pool, workerConfigPath);
    pool.stdout!.on("data", (c) => (poolLog += c));
    pool.stderr!.on("data", (c) => (poolLog += c));
    await Promise.race([
      once(pool.stdout!, "data"),
      once(pool, "exit").then(([code]) => { throw new Error(`restarted pool exited (${code}) before starting:\n${poolLog}`); }),
    ]);
    await waitFor(async () => SETTLED.includes((await jobState(taskC.id)).state), "task C to settle after restart", 60_000);
    expect(SETTLED_OK.includes((await jobState(taskC.id)).state), poolLog).toBe(true);
    // More than one attempt proves the restarted pool, not the killed one, finished the job.
    expect((await jobState(taskC.id)).attempt).toBeGreaterThan(1);
  } finally {
    pool?.kill("SIGKILL");
    if (pool) await once(pool, "exit").catch(() => {});
    host.kill("SIGINT");
    await once(host, "exit");
    await rm(base, { recursive: true, force: true });
  }
}, 180_000);
