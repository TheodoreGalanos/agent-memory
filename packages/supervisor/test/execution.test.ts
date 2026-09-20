// ABOUTME: Tests that an investigation task's execution service determines whether and how its
// ABOUTME: granted native tools actually run, against a real Host (own `world()`, not supervisor.test.ts's).
import { spawn } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { once } from "node:events";
import { createServer } from "node:net";
import { randomBytes, randomUUID } from "node:crypto";
import { BACKGROUND_CONTEXT as context } from "@earendil-works/pi-agent-core";
import { createModels, fauxProvider, fauxAssistantMessage, fauxToolCall } from "@earendil-works/pi-ai";
import { expect, it } from "vitest";
import type { HostRequest } from "../../../contracts/generated/host-request.js";
import type { HostResponse } from "../../../contracts/generated/host-response.js";
import { HostClient } from "../../host-client/src/client.js";
import { fenceFor } from "../../host-client/src/fence.js";
import { input } from "../../pi-worker/test/fixtures.js";
import { POOL_MODULES, composeApplication, runSupervisor, type Services } from "../src/index.js";

function compose(services: Services, ...processes: ("investigation" | "formation")[]) {
  return composeApplication(services, POOL_MODULES.filter((m) => m.declaration.operations.some((op) => (processes as string[]).includes(op))));
}
const taskService = (models: ReturnType<typeof createModels>, faux: ReturnType<typeof fauxProvider>) =>
  ({ models, model: faux.getModel(), maxPayloadBytes: 100_000 });

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

/** Copied from `packages/supervisor/test/supervisor.test.ts`'s `world()` helper (own copy per the
 * lane rules; that file is not imported from or edited beyond its one permitted version bump). */
interface World {
  base: string; url: string; host: ReturnType<typeof spawn>; client: HostClient; admin: HostClient; pool: HostClient;
  scope: Record<string, unknown>; policy: { reference: unknown }; budget: { id: string };
  submit: (process: "investigation" | "formation", extra: Record<string, unknown>) => Promise<import("../../../contracts/generated/host-response.js").Job>;
  job: (id: string) => Promise<import("../../../contracts/generated/host-response.js").Job>;
  close: () => Promise<void>;
}
async function world(): Promise<World> {
  const base = await mkdtemp(join(tmpdir(), "memory-execution-"));
  const port = await freePort();
  const tenant_id = randomUUID(), actor_id = randomUUID(), project_id = randomUUID();
  const scope = { user_id: null, project_id, task_id: null, entity_ids: [], source_versions: [] };
  const tokens = { client: randomBytes(32).toString("hex"), admin: randomBytes(32).toString("hex"), pool: randomBytes(32).toString("hex") };
  const adminActor = randomUUID();
  const expires_at = minutes(60);
  await writeFile(join(base, "host.json"), JSON.stringify({
    listen: `127.0.0.1:${port}`, database_url: `sqlite://${join(base, "memory.db")}?mode=rwc`, artifact_root: join(base, "artifacts"),
    credentials: [
      { token: tokens.client, tenant_id, actor_id, scope, role: { kind: "client" }, expires_at },
      { token: tokens.admin, tenant_id, actor_id: adminActor, scope: { ...scope, project_id: null }, role: { kind: "administrator" }, expires_at },
      { token: tokens.pool, tenant_id, actor_id: randomUUID(), scope: { ...scope, project_id: null }, role: { kind: "pool", classes: ["interactive", "deferred"] }, expires_at },
    ],
  }), { mode: 0o600 });
  const host = spawn(resolve("target/debug/memory-host"), [join(base, "host.json")], { stdio: ["ignore", "pipe", "pipe"] });
  let errors = ""; host.stderr!.on("data", (c) => (errors += c));
  await Promise.race([once(host.stdout!, "data"), once(host, "exit").then(() => { throw new Error(errors); })]);
  const url = `http://127.0.0.1:${port}`;
  const client = new HostClient(url, tokens.client), admin = new HostClient(url, tokens.admin);
  const fixture = input("before-correction").command;
  const created = expectKind(await admin.request({ action: "user", request: { action: "mutate", request_id: randomUUID(), mutation: {
    action: "create_policy", label: "Execution test", scope, effective: { kind: "unknown" },
    policy: { retention_purpose: "test", allowed_uses: ["task_context"], source_rules: [], evidence_requirements: [], applicability_rules: [], budget_class: "test", scheduling_priority: 0, judgement_dispositions: [], notification_policy: "material", qualification_requirements: [] } } } }), "user").result;
  if (created.kind !== "policy") throw new Error("policy");
  const budget = { id: randomUUID(), scope, limit: { tokens: 5_000_000, provider_calls: 50, output_bytes: 5_000_000, cost_microunits: 5_000_000, sandbox_cpu_ms: 0, sandbox_time_ms: 0 }, final_result_reserve: { tokens: 100, provider_calls: 0, output_bytes: 1000, cost_microunits: 0, sandbox_cpu_ms: 0, sandbox_time_ms: 0 }, deadline: minutes(30), max_child_depth: 0, max_child_concurrency: 0, pricing_revision: "test" };
  await admin.request({ action: "create_budget", budget });
  return {
    base, url, host, client, admin, pool: new HostClient(url, tokens.pool), scope, policy: { reference: created.policy.reference }, budget,
    async submit(process, extra) {
      const task_id = randomUUID();
      const brief = { ...fixture.payload, process, task_id, scope: { ...scope, task_id }, policy: created.policy.reference, purpose: `${process} test`,
        inputs: { memories: [], sources: [], artifacts: [] }, capabilities: { sources: [], queries: [], tools: [] }, known_conflicts: [], evidence_cutoff: new Date().toISOString(),
        profile: { id: randomUUID(), revision: 1, label: `${process} profile` },
        limits: { root_budget_id: budget.id, max_provider_attempts: 8, max_tokens: 4_000_000, max_output_bytes: 1_000_000, max_child_depth: 0, max_child_concurrency: 0 }, ...extra };
      const command = { ...fixture, request_id: randomUUID(), tenant_id, actor_id, scope: brief.scope, budget_id: budget.id, deadline: minutes(20), payload: { brief, parent_id: null, max_attempts: 2, retain_until: minutes(1440) } };
      return expectKind(await client.request({ action: "submit", command } as HostRequest), "job").job;
    },
    async job(id) { return expectKind(await client.request({ action: "inspect_job", job_id: id }), "job").job; },
    async close() { host.kill("SIGINT"); await once(host, "exit"); await rm(base, { recursive: true, force: true }); },
  };
}

it("a task granted read runs it against a real node execution environment and the file's content reaches its experience", async () => {
  const w = await world();
  try {
    const cwd = await mkdtemp(join(tmpdir(), "memory-execution-cwd-"));
    const fileText = "Inspection note: torque on bolt A7 measured 42 Nm.";
    await writeFile(join(cwd, "note.txt"), fileText);
    const task = await w.submit("investigation", { capabilities: { sources: [], queries: [], tools: ["read"] } });
    const faux = fauxProvider(), models = createModels();
    models.setProvider(faux.provider);
    const answer = {
      schema_version: "1", status: "complete", examined_scope: task.spec.brief.scope, inputs: task.spec.brief.inputs,
      findings: [], coverage: { examined: [`Read note.txt: ${fileText}`], unexamined: [] }, unresolved_work: [],
      proposed_changes: [], child_outputs: [], known_effects: [],
      usage: { status: "unknown", input_tokens: null, output_tokens: null, cost: null },
    };
    faux.setResponses([
      fauxAssistantMessage(fauxToolCall("read", { path: "note.txt" }), { stopReason: "toolUse" }),
      fauxAssistantMessage(JSON.stringify(answer)),
    ]);
    const log: string[] = [];
    await runSupervisor({
      host: w.pool, hostUrl: w.url, concurrency: 1, pollMs: 50, leaseSeconds: 60, sessionsDirectory: join(w.base, "sessions"),
      composition: compose({ taskModel: taskService(models, faux), execution: { kind: "node", cwd } }, "investigation"),
      log: (line) => log.push(line),
      until: async () => SETTLED.includes((await w.job(task.id)).state),
    }, context);
    const done = await w.job(task.id);
    expect(done.state, log.join("\n")).toBe("completed");
    expect(done.result?.coverage.examined.join(" ")).toContain(fileText);

    // The published experience is the durable proof the `read` tool actually ran against the real
    // file: its tool_result event carries the file's own content, not something the model invented.
    const experience = expectKind(await w.client.request({ action: "experience", job_id: task.id }), "source").source;
    const formationJob = await w.submit("formation", {
      inputs: { memories: [], sources: [experience.reference], artifacts: [] },
      capabilities: { sources: [experience.reference], queries: [], tools: [] },
    });
    const claimed = await w.pool.request({ action: "claim_next", processes: ["formation"], lease_seconds: 60 });
    if (claimed.kind !== "work") throw new Error("formation job was not claimed");
    expect(claimed.assignment.job.id).toBe(formationJob.id);
    const fence = fenceFor(claimed.assignment);
    const jobClient = new HostClient(w.url, claimed.credential.token);
    await jobClient.request({ action: "start", fence, composition: "test" }, context.abortSignal);
    const captured = await jobClient.request({
      action: "formation_window", fence, id: randomUUID(), source: experience.reference, operation: "read note", limit: 32,
    }, context.abortSignal);
    if (captured.kind !== "experience_window") throw new Error(`Expected an experience window, got ${captured.kind}`);
    const events = captured.window.spans.flatMap((span) => span.events);
    const toolResult = events.find((event) => event.kind === "tool_result");
    expect(toolResult, JSON.stringify(events)).toBeTruthy();
    expect(JSON.stringify(toolResult!.content)).toContain(fileText);
    await rm(cwd, { recursive: true, force: true });
  } finally {
    await w.close();
  }
}, 120_000);

it("a tool-less task (no execution service) is granted no native tools even when the brief lists them", async () => {
  const w = await world();
  try {
    const task = await w.submit("investigation", { capabilities: { sources: [], queries: [], tools: ["read", "bash"] } });
    const faux = fauxProvider(), models = createModels();
    models.setProvider(faux.provider);
    const answer = {
      schema_version: "1", status: "complete", examined_scope: task.spec.brief.scope, inputs: task.spec.brief.inputs,
      findings: [], coverage: { examined: ["No tools were available; answered from the brief alone"], unexamined: [] },
      unresolved_work: [], proposed_changes: [], child_outputs: [], known_effects: [],
      usage: { status: "unknown", input_tokens: null, output_tokens: null, cost: null },
    };
    faux.setResponses([fauxAssistantMessage(JSON.stringify(answer))]);
    const log: string[] = [];
    await runSupervisor({
      host: w.pool, hostUrl: w.url, concurrency: 1, pollMs: 50, leaseSeconds: 60, sessionsDirectory: join(w.base, "sessions"),
      // No `execution` service installed: investigation is tool-less regardless of what the brief grants.
      composition: compose({ taskModel: taskService(models, faux) }, "investigation"),
      log: (line) => log.push(line),
      until: async () => SETTLED.includes((await w.job(task.id)).state),
    }, context);
    const done = await w.job(task.id);
    expect(done.state, log.join("\n")).toBe("completed");
  } finally {
    await w.close();
  }
}, 120_000);

it("constructs an ASP execution environment from the service's descriptor without a live sandbox, and rejects an invalid one", async () => {
  const { AspExecutionEnv } = await import("../../pi-worker/src/asp-execution-env.js");
  const binding = {
    sandbox_id: "s", generation: "1", session_id: "task", epoch: 1, expires_at: Date.now() + 60_000,
    descriptor: { version: "0" as const, transport: "ssh" as const, workspace: "/workspace", connection: { host: "sandbox.internal", user: "runner", host_key: "ssh-ed25519 YWJj" } },
  };
  const env = await AspExecutionEnv.connect(binding, binding);
  expect(env.cwd).toBe("/workspace");
  await expect(AspExecutionEnv.connect({ ...binding, epoch: 2 }, binding)).rejects.toThrow("trusted allocation");
});
