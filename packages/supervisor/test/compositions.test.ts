// ABOUTME: The composition matrix: a small job run through each of Pi alone, Pi+Jev, read-only memory,
// ABOUTME: capture-only, full learning and full-minus-one, each against its own real Host and pool.
import { spawn } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { once } from "node:events";
import { createServer } from "node:net";
import { randomBytes, randomUUID } from "node:crypto";
import { BACKGROUND_CONTEXT as context, withCancel } from "@earendil-works/pi-agent-core";
import { createModels, fauxProvider, fauxAssistantMessage } from "@earendil-works/pi-ai";
import { expect, it } from "vitest";
import type { HostRequest, RecordDraft, Scope } from "../../../contracts/generated/host-request.js";
import type { HostResponse } from "../../../contracts/generated/host-response.js";
import type { JudgementPacket } from "../../../contracts/generated/judgement-packet.js";
import type { FormationEntry } from "../../../contracts/generated/formation-window.js";
import { HostClient } from "../../host-client/src/client.js";
import { input, loadJson } from "../../pi-worker/test/fixtures.js";
import { GenerativeProvider, type JudgementProvider } from "../../judgement/src/providers.js";
import { maximum } from "../../judgement/test/fixtures.js";
import { POOL_MODULES, composeApplication, runSupervisor, type Services, type ProcessHandler } from "../src/index.js";
import { toolName } from "../../pi-worker/src/harness.js";
import { investigationModule } from "../src/investigation.js";
import { decisionModule } from "../src/decision.js";
import { activationModule } from "../src/activation.js";
import { formationModule } from "../src/formation.js";

type LearningProcess = "investigation" | "formation" | "consolidation" | "evaluation" | "maintenance" | "activation";

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

interface World {
  base: string; url: string; host: ReturnType<typeof spawn>; client: HostClient; admin: HostClient; pool: HostClient;
  scope: Scope; budget: { id: string }; profile: { id: string; revision: number; label: string };
  submit: (process: LearningProcess, extra: Record<string, unknown>) => Promise<import("../../../contracts/generated/host-response.js").Job>;
  job: (id: string) => Promise<import("../../../contracts/generated/host-response.js").Job>;
  contribute: (record: Partial<RecordDraft> & Pick<RecordDraft, "content" | "label">, task_id?: string) => Promise<import("../../../contracts/generated/host-response.js").MemoryVersion>;
  ingestCapture: (events: unknown[]) => Promise<{ source_id: string; revision: string }>;
  close: () => Promise<void>;
}
/** Own copy for this test file: `packages/supervisor/test/supervisor.test.ts` must not be imported or edited. */
async function world(): Promise<World> {
  const base = await mkdtemp(join(tmpdir(), "memory-compositions-"));
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
  const client = new HostClient(url, tokens.client), admin = new HostClient(url, tokens.admin), pool = new HostClient(url, tokens.pool);
  const fixture = input("before-correction").command;
  const created = expectKind(await admin.request({ action: "user", request: { action: "mutate", request_id: randomUUID(), mutation: {
    action: "create_policy", label: "Composition test", scope, effective: { kind: "unknown" },
    policy: { retention_purpose: "test", allowed_uses: ["task_context"], source_rules: [], evidence_requirements: [], applicability_rules: [], budget_class: "test", scheduling_priority: 0, judgement_dispositions: [], notification_policy: "material", qualification_requirements: [] } } } }), "user").result;
  if (created.kind !== "policy") throw new Error("policy");
  const budget = { id: randomUUID(), scope, limit: { tokens: 5_000_000, provider_calls: 200, output_bytes: 5_000_000, cost_microunits: 5_000_000, sandbox_cpu_ms: 0, sandbox_time_ms: 0 }, final_result_reserve: { tokens: 100, provider_calls: 0, output_bytes: 1000, cost_microunits: 0, sandbox_cpu_ms: 0, sandbox_time_ms: 0 }, deadline: minutes(30), max_child_depth: 1, max_child_concurrency: 2, pricing_revision: "test" };
  await admin.request({ action: "create_budget", budget });
  const profile = { id: randomUUID(), revision: 1, label: "composition-profile" };
  const submit: World["submit"] = async (process, extra) => {
    const task_id = randomUUID();
    const brief = { ...fixture.payload, process, task_id, scope: { ...scope, task_id }, policy: created.policy.reference, purpose: `${process} test`,
      inputs: { memories: [], sources: [], artifacts: [] }, capabilities: { sources: [], queries: [], tools: [] }, known_conflicts: [], evidence_cutoff: new Date().toISOString(),
      profile,
      limits: { root_budget_id: budget.id, max_provider_attempts: 8, max_tokens: 4_000_000, max_output_bytes: 1_000_000, max_child_depth: 1, max_child_concurrency: 2 }, ...extra };
    const command = { ...fixture, request_id: randomUUID(), tenant_id, actor_id, scope: brief.scope, budget_id: budget.id, deadline: minutes(20), payload: { brief, parent_id: null, max_attempts: 2, retain_until: minutes(1440) } };
    return expectKind(await client.request({ action: "submit", command } as HostRequest), "job").job;
  };
  return {
    base, url, host, client, admin, pool, scope, budget, profile,
    submit,
    async job(id) { return expectKind(await client.request({ action: "inspect_job", job_id: id }), "job").job; },
    async contribute(record, task_id) {
      const draft: RecordDraft = {
        scope: task_id ? { ...scope, task_id } : scope, origin: "observed", evidential_status: "attributed_statement", availability: "routine", qualification: { status: "candidate" },
        valid_time: { kind: "unknown" }, source_locators: [], derived_from: [],
        decision: { policy: created.policy.reference, action: "retain", reason: "Contributed for the composition matrix", constraints: [], required_evidence: [], expires_at: null },
        ...record,
      };
      const result = expectKind(await client.request({ action: "user", request: { action: "mutate", request_id: randomUUID(), mutation: { action: "contribute", record: draft } } }), "user").result;
      if (result.kind !== "memory") throw new Error(`Expected a contributed memory, got ${result.kind}`);
      return result.memory;
    },
    async ingestCapture(events) {
      const source = { source_id: randomUUID(), revision: "1" };
      await client.request({ action: "ingest_source", source: { reference: source, label: "Capture", scope, kind: "tool_events", owner: "test", acquired_at: new Date().toISOString(), acquisition_method: "test", precedence: null, snapshot_artifact: null }, content: { schema_version: "memory-tool-events/1", events } });
      return source;
    },
    async close() { host.kill("SIGINT"); await once(host, "exit"); await rm(base, { recursive: true, force: true }); },
  };
}

/** The judgement families formation and admission-time activation ask, answered deterministically. */
function packetChoice(packet: JudgementPacket, key: string): string {
  if (key === "J06.direct") return "yes";
  if (key.startsWith("J06.")) return "no";
  if (key === "J07.assessment") return "relevant";
  if (key === "J08.assessment") return "established";
  if (key === "J30.role") {
    const span = packet.evidence.find((e) => e.name === "span")?.content as { kind: string; events: { content: { role?: string } }[] } | undefined;
    return span && (span.kind === "exchange" || span.events[0]?.content.role === "assistant") ? "knowledge" : "none";
  }
  const entry = packet.evidence.find((e) => e.name === "candidate")?.content as unknown as FormationEntry | undefined;
  if (entry) {
    const choices: Record<string, string> = { "J01.assessment": entry.event.evidential_status, "J01.faithfulness": "yes", "J02.assessment": "supports", "J03.assessment": "boundary", "J04.assessment": "conditional_method", "J05.assessment": "commitment", "J27.assessment": "task_local" };
    if (key in choices) return choices[key];
  }
  throw new Error(`Composition test has no scripted answer for ${key}; packet: ${JSON.stringify(packet.questions.map((q) => q.definition.id))}`);
}
function scriptedJudgement(faux: ReturnType<typeof fauxProvider>, models: ReturnType<typeof createModels>): JudgementProvider {
  const generative = new GenerativeProvider("reference", models, faux.getModel(), maximum, 2048, faux.getModel().id);
  return {
    id: generative.id, model: generative.model, release: generative.release, maximum, distributions: false,
    async evaluate(packet: JudgementPacket, signal: AbortSignal) {
      const answers = Object.fromEntries(packet.questions.flatMap((d) => d.definition.questions.map((q) => [`${d.definition.id}.${q.key}`, { type: "choice", choice: packetChoice(packet, `${d.definition.id}.${q.key}`) }])));
      faux.setResponses([fauxAssistantMessage(JSON.stringify({ answers }))]);
      return generative.evaluate(packet, signal);
    },
  };
}

/** A no-tool investigation task: a single reply, no admission retrieval expected unless judgement
 * is installed (in which case an activation log line is expected too). */
function taskReply(brief: { scope: unknown; inputs: unknown }) {
  return {
    schema_version: "1", status: "complete", examined_scope: brief.scope, inputs: brief.inputs,
    findings: [], coverage: { examined: ["Answered from the task model alone"], unexamined: [] }, unresolved_work: [],
    proposed_changes: [], child_outputs: [], known_effects: [], usage: { status: "unknown", input_tokens: null, output_tokens: null, cost: null },
  };
}

it("composeApplication accepts a provider swap but rejects a missing required service before any claim is made", () => {
  const models = createModels(), faux = fauxProvider();
  models.setProvider(faux.provider);
  const primary = scriptedJudgement(faux, models);
  const alternate: JudgementProvider = { ...primary, id: "alternate-provider" };
  // A different provider id satisfying the same `judgement` capability is a valid alternative.
  expect(() => composeApplication({ judgement: { primary: alternate, maximum } }, [activationModule])).not.toThrow();
  expect(() => composeApplication({ judgement: { primary, maximum } }, [activationModule])).not.toThrow();
  // Removing a required service (activation requires judgement) is rejected before any work is claimed.
  expect(() => composeApplication({}, [activationModule])).toThrow(/requires judgement/);
  // decisionModule also requires judgement; composing it without one is rejected the same way.
  expect(() => composeApplication({}, [decisionModule])).toThrow(/requires judgement/);
});

it("Pi alone: a tool-less, judgement-less investigation completes with no admission retrieval and no decision tool available", async () => {
  const w = await world();
  try {
    const job = await w.submit("investigation", {});
    const faux = fauxProvider(), models = createModels();
    models.setProvider(faux.provider);
    faux.setResponses([fauxAssistantMessage(JSON.stringify(taskReply(job.spec.brief)))]);
    const composition = composeApplication({ taskModel: taskService(models, faux) }, [investigationModule]);
    expect(composition.modules.map((m) => m.id)).toEqual(["investigation"]);
    expect(composition.tools.some((t) => toolName(t) === "decide")).toBe(false);
    const log: string[] = [];
    await runSupervisor({
      host: w.pool, hostUrl: w.url, concurrency: 1, pollMs: 50, leaseSeconds: 60, sessionsDirectory: join(w.base, "sessions"),
      composition, log: (l) => log.push(l), until: async () => SETTLED.includes((await w.job(job.id)).state),
    }, context);
    expect((await w.job(job.id)).state, log.join("\n")).toBe("completed");
    expect(log.some((l) => l.includes("activation:")), log.join("\n")).toBe(false);
  } finally {
    await w.close();
  }
}, 60_000);

it("Pi+Jev: judgement installed offers the decide tool, but with no memory processes composed nothing is ever formed", async () => {
  const w = await world();
  try {
    const job = await w.submit("investigation", {});
    const taskFaux = fauxProvider(), taskModels = createModels();
    taskModels.setProvider(taskFaux.provider);
    taskFaux.setResponses([fauxAssistantMessage(JSON.stringify(taskReply(job.spec.brief)))]);
    const judgementFaux = fauxProvider(), judgementModels = createModels();
    judgementModels.setProvider(judgementFaux.provider);
    const services: Services = { taskModel: taskService(taskModels, taskFaux), judgement: { primary: scriptedJudgement(judgementFaux, judgementModels), maximum } };
    const composition = composeApplication(services, [investigationModule, decisionModule]);
    expect(composition.tools.some((t) => toolName(t) === "decide")).toBe(true);
    const log: string[] = [];
    await runSupervisor({
      host: w.pool, hostUrl: w.url, concurrency: 1, pollMs: 50, leaseSeconds: 60, sessionsDirectory: join(w.base, "sessions"),
      composition, log: (l) => log.push(l), until: async () => SETTLED.includes((await w.job(job.id)).state),
    }, context);
    expect((await w.job(job.id)).state, log.join("\n")).toBe("completed");
    const browse = expectKind(await w.client.request({ action: "user", request: { action: "browse", query: { include_inactive: true, limit: 50 } } }), "user").result;
    if (browse.kind !== "records") throw new Error("records");
    expect(browse.records).toHaveLength(0);
  } finally {
    await w.close();
  }
}, 60_000);

it("read-only memory: admission retrieval runs, but nothing is ever formed without formationModule composed", async () => {
  const w = await world();
  try {
    await w.contribute({ label: "Beam clearance", content: { family: "knowledge", statement: "Clearance under beam B3 is 2.0 m.", uncertainty: [], examined_coverage: [] } });
    const job = await w.submit("investigation", { purpose: "What clearance is available under beam B3?" });
    const taskFaux = fauxProvider(), taskModels = createModels();
    taskModels.setProvider(taskFaux.provider);
    taskFaux.setResponses([fauxAssistantMessage(JSON.stringify(taskReply(job.spec.brief)))]);
    const judgementFaux = fauxProvider(), judgementModels = createModels();
    judgementModels.setProvider(judgementFaux.provider);
    const services: Services = { taskModel: taskService(taskModels, taskFaux), judgement: { primary: scriptedJudgement(judgementFaux, judgementModels), maximum } };
    const composition = composeApplication(services, [investigationModule, activationModule]);
    const log: string[] = [];
    await runSupervisor({
      host: w.pool, hostUrl: w.url, concurrency: 1, pollMs: 50, leaseSeconds: 60, sessionsDirectory: join(w.base, "sessions"),
      composition, log: (l) => log.push(l), until: async () => SETTLED.includes((await w.job(job.id)).state),
    }, context);
    expect((await w.job(job.id)).state, log.join("\n")).toBe("completed");
    // Admission-time retrieval ran (investigation's own optional judgement dependency).
    expect(log.some((l) => l.includes("activation:")), log.join("\n")).toBe(true);
    // Nothing new is ever formed: only the one record contributed before the job ran exists.
    const browse = expectKind(await w.client.request({ action: "user", request: { action: "browse", query: { include_inactive: true, limit: 50 } } }), "user").result;
    if (browse.kind !== "records") throw new Error("records");
    expect(browse.records).toHaveLength(1);
  } finally {
    await w.close();
  }
}, 60_000);

it("capture-only: formation retains records, and a module the pool never composed leaves its scheduled work queued rather than lost", async () => {
  const w = await world();
  try {
    const capture = loadJson("evals/formation/capture.json") as { events: unknown[] };
    const source = await w.ingestCapture(capture.events.slice(0, 2));
    const job = await w.submit("formation", { inputs: { memories: [], sources: [source], artifacts: [] }, capabilities: { sources: [source], queries: [], tools: [] } });
    const judgementFaux = fauxProvider(), judgementModels = createModels();
    judgementModels.setProvider(judgementFaux.provider);
    const composition = composeApplication({ judgement: { primary: scriptedJudgement(judgementFaux, judgementModels), maximum } }, [formationModule]);
    expect(composition.modules.map((m) => m.id).sort()).toEqual(["formation"]);
    const log: string[] = [];
    await runSupervisor({
      host: w.pool, hostUrl: w.url, concurrency: 1, pollMs: 50, leaseSeconds: 60, sessionsDirectory: join(w.base, "sessions"),
      composition, log: (l) => log.push(l), until: async () => SETTLED.includes((await w.job(job.id)).state),
    }, context);
    const done = await w.job(job.id);
    expect(["completed", "partial"], log.join("\n")).toContain(done.state);
    const browse = expectKind(await w.client.request({ action: "user", request: { action: "browse", query: { include_inactive: false, limit: 50 } } }), "user").result;
    if (browse.kind !== "records") throw new Error("records");
    expect(browse.records.length).toBeGreaterThan(0);

    // A correction to a formed record schedules a maintenance job (the Host's own scheduler runs
    // regardless of what the pool composes); this pool never composed maintenanceModule, so it can
    // never claim it. The trigger's own data (the correction itself) is unaffected either way.
    const target = browse.records[0];
    const corrected = structuredClone(target.record);
    if (corrected.content.family === "knowledge") corrected.content.statement = `${corrected.content.statement} (corrected)`;
    await w.client.request({ action: "user", request: { action: "mutate", request_id: randomUUID(), mutation: { action: "correct", expected: target.reference, record: corrected } } });
    const scheduledMaintenance = async () => {
      const page = expectKind(await w.client.request({ action: "events", after: 0, limit: 500 }), "events").page;
      for (const event of page.events.filter((e) => e.kind === "job_accepted")) {
        const candidate = await w.job(event.resource_id);
        if (candidate.spec.brief.process === "maintenance") return candidate;
      }
      return undefined;
    };
    const started = Date.now();
    let maintenanceJob = await scheduledMaintenance();
    while (!maintenanceJob) {
      if (Date.now() - started > 30_000) throw new Error("the Host did not schedule maintenance for the correction");
      await new Promise((resolve) => setTimeout(resolve, 500));
      maintenanceJob = await scheduledMaintenance();
    }
    // The pool (no maintenanceModule) never claims it; give it a fair chance to (wrongly) do so.
    await runSupervisor({
      host: w.pool, hostUrl: w.url, concurrency: 1, pollMs: 50, leaseSeconds: 60, sessionsDirectory: join(w.base, "sessions"),
      composition, log: () => {},
      until: (() => { let ticks = 0; return async () => ++ticks > 10; })(),
    }, context);
    expect((await w.job(maintenanceJob.id)).state).toBe("queued");
    // The corrected record itself is intact regardless: it is readable at its new revision.
    const history = expectKind(await w.client.request({ action: "user", request: { action: "history", memory_id: target.reference.memory_id, after_revision: 0, limit: 10 } }), "user").result;
    if (history.kind !== "history") throw new Error("history");
    expect(history.records).toHaveLength(2);
  } finally {
    await w.close();
  }
}, 60_000);

it("full learning composes every process module the pool ships, and dropping one still composes the rest", () => {
  const services: Services = {
    taskModel: taskService(createModels(), fauxProvider()),
    judgement: { primary: scriptedJudgement(fauxProvider(), createModels()), maximum },
  };
  const full = composeApplication(services, POOL_MODULES);
  expect(full.modules.map((m) => m.id).sort()).toEqual(
    ["activation", "consolidation", "decision", "evaluation", "formation", "investigation", "maintenance"].sort(),
  );
  const minusMaintenance = composeApplication(services, POOL_MODULES.filter((m) => m.declaration.id !== "maintenance"));
  expect(minusMaintenance.modules.map((m) => m.id)).not.toContain("maintenance");
  expect(minusMaintenance.modules).toHaveLength(full.modules.length - 1);
});

it("the pool drains outstanding work for drainMs after shutdown instead of cancelling it immediately", async () => {
  let cancelled = false, started = false;
  const handler: ProcessHandler = {
    process: "investigation",
    async run(run) {
      started = true;
      await new Promise<void>((resolve) => {
        run.context.abortSignal?.addEventListener("abort", () => { cancelled = true; resolve(); });
      });
    },
  };
  const composition = composeApplication({}, [{ declaration: { id: "test", version: "1", requires: [], operations: ["investigation"] }, attach: () => [handler] }]);
  const assignment = {
    epoch: 1, owner_id: randomUUID(), expires_at: minutes(5),
    job: { id: randomUUID(), root_id: randomUUID(), depth: 0, attempt: 1, session_id: randomUUID(), operation_id: randomUUID(),
      state: "leased", deadline: minutes(5), cancel_requested: false,
      spec: { brief: { ...input("before-correction").command.payload, process: "investigation" }, max_attempts: 2, retain_until: minutes(60) } },
  };
  let claimed = false;
  const host = {
    async request(command: { action: string }) {
      if (command.action === "claim_next" && !claimed) { claimed = true; return { kind: "work", assignment, credential: { actor_id: randomUUID(), expires_at: assignment.expires_at, job_id: assignment.job.id, scope: assignment.job.spec.brief.scope, token: "t".repeat(40) } }; }
      if (command.action === "claim_next") return { kind: "idle" };
      return { kind: "ok" };
    },
  };
  const { context: runContext, cancel } = withCancel(context);
  const done = runSupervisor({
    host: host as never, connect: () => host as never, concurrency: 1, pollMs: 10, leaseSeconds: 60, drainMs: 500,
    sessionsDirectory: join(tmpdir(), `compositions-drain-${randomUUID()}`),
    composition, log: () => {},
  }, runContext);
  const settleStarted = Date.now();
  while (!started) {
    if (Date.now() - settleStarted > 5_000) throw new Error("handler never started");
    await new Promise((r) => setTimeout(r, 10));
  }
  cancel();
  // Immediately after cancelling, the running work has not been cancelled yet: it is draining.
  expect(cancelled).toBe(false);
  await done;
  expect(cancelled).toBe(true);
}, 15_000);
