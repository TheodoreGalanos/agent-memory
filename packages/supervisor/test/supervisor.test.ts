// ABOUTME: End-to-end test of the worker pool against a real Host: a pool credential claims an
// ABOUTME: investigation and a formation job, drives them with scripted providers, and settles both.
import { spawn } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { once } from "node:events";
import { createServer } from "node:net";
import { randomBytes, randomUUID } from "node:crypto";
import { BACKGROUND_CONTEXT as context } from "@earendil-works/pi-agent-core";
import { createModels, fauxProvider, fauxAssistantMessage } from "@earendil-works/pi-ai";
import { expect, it } from "vitest";
import type { HostRequest, RecordDraft } from "../../../contracts/generated/host-request.js";
import type { HostResponse } from "../../../contracts/generated/host-response.js";
import type { JudgementPacket } from "../../../contracts/generated/judgement-packet.js";
import type { FormationEntry } from "../../../contracts/generated/formation-window.js";
import { HostClient } from "../../pi-worker/src/host-client.js";
import { input, loadJson } from "../../pi-worker/test/fixtures.js";
import { GenerativeProvider, type JudgementProvider } from "../../judgement/src/providers.js";
import { maximum } from "../../judgement/test/fixtures.js";
import { runSupervisor } from "../src/index.js";

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


interface World {
  base: string; url: string; host: ReturnType<typeof spawn>; client: HostClient; admin: HostClient; pool: HostClient;
  scope: Record<string, unknown>; policy: { reference: unknown }; budget: { id: string };
  submit: (process: "investigation" | "formation", extra: Record<string, unknown>) => Promise<import("../../../contracts/generated/host-response.js").Job>;
  job: (id: string) => Promise<import("../../../contracts/generated/host-response.js").Job>;
  ingestCapture: () => Promise<{ source_id: string; revision: string }>;
  close: () => Promise<void>;
}
async function world(): Promise<World> {
  const base = await mkdtemp(join(tmpdir(), "memory-supervisor-"));
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
  let errors = ""; host.stderr!.on("data", (c) => (errors += c));
  await Promise.race([once(host.stdout!, "data"), once(host, "exit").then(() => { throw new Error(errors); })]);
  const url = `http://127.0.0.1:${port}`;
  const client = new HostClient(url, tokens.client), admin = new HostClient(url, tokens.admin);
  const fixture = input("before-correction").command;
  const created = expectKind(await admin.request({ action: "user", request: { action: "mutate", request_id: randomUUID(), mutation: {
    action: "create_policy", label: "Pool test", scope, effective: { kind: "unknown" },
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
    async ingestCapture() {
      const capture = loadJson("evals/formation/capture.json") as { events: unknown[] };
      const source = { source_id: randomUUID(), revision: "1" };
      await client.request({ action: "ingest_source", source: { reference: source, label: "Capture", scope, kind: "tool_events", owner: "test", acquired_at: new Date().toISOString(), acquisition_method: "test", precedence: null, snapshot_artifact: null }, content: { schema_version: "memory-tool-events/1", events: capture.events.slice(0, 2) } });
      return source;
    },
    async close() { host.kill("SIGINT"); await once(host, "exit"); await rm(base, { recursive: true, force: true }); },
  };
}
/** Scripted judgement reference answering from each candidate's own evidential status. */
function scriptedReference(faux: ReturnType<typeof fauxProvider>, models: ReturnType<typeof createModels>, onCall?: () => void): JudgementProvider {
  const generative = new GenerativeProvider("reference", models, faux.getModel(), maximum, 2048, faux.getModel().id);
  return {
    id: generative.id, model: generative.model, release: generative.release, maximum, distributions: false,
    async evaluate(packet: JudgementPacket, signal: AbortSignal) {
      onCall?.();
      const entry = packet.evidence.find((e) => e.name === "candidate")!.content as unknown as FormationEntry;
      const choices: Record<string, string> = { "J01.assessment": entry.event.evidential_status, "J01.faithfulness": "yes", "J02.assessment": "supports", "J03.assessment": "correction", "J04.assessment": "conditional_method", "J05.assessment": "commitment", "J27.assessment": "task_local" };
      const answers = Object.fromEntries(packet.questions.flatMap((d) => d.definition.questions.map((q) => [`${d.definition.id}.${q.key}`, { type: "choice", choice: choices[`${d.definition.id}.${q.key}`] }])));
      faux.setResponses([fauxAssistantMessage(JSON.stringify({ answers }))]);
      return generative.evaluate(packet, signal);
    },
  };
}
const SETTLED = ["completed", "partial", "failed"];

it("a pool claims, drives and settles investigation and formation jobs through a real Host", async () => {
  const base = await mkdtemp(join(tmpdir(), "memory-supervisor-"));
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
  let errors = ""; host.stderr!.on("data", (c) => (errors += c));
  await Promise.race([once(host.stdout!, "data"), once(host, "exit").then(() => { throw new Error(errors); })]);
  const url = `http://127.0.0.1:${port}`;
  const client = new HostClient(url, tokens.client), admin = new HostClient(url, tokens.admin);
  try {
    const fixture = input("before-correction").command;
    const policy = expectKind(await admin.request({ action: "user", request: { action: "mutate", request_id: randomUUID(), mutation: {
      action: "create_policy", label: "Pool test", scope, effective: { kind: "unknown" },
      policy: { retention_purpose: "test", allowed_uses: ["task_context"], source_rules: [], evidence_requirements: [], applicability_rules: [], budget_class: "test", scheduling_priority: 0, judgement_dispositions: [], notification_policy: "material", qualification_requirements: [] } } } }), "user").result;
    if (policy.kind !== "policy") throw new Error("policy");
    const budget = { id: randomUUID(), scope, limit: { tokens: 5_000_000, provider_calls: 50, output_bytes: 5_000_000, cost_microunits: 5_000_000, sandbox_cpu_ms: 0, sandbox_time_ms: 0 }, final_result_reserve: { tokens: 100, provider_calls: 0, output_bytes: 1000, cost_microunits: 0, sandbox_cpu_ms: 0, sandbox_time_ms: 0 }, deadline: minutes(30), max_child_depth: 0, max_child_concurrency: 0, pricing_revision: "test" };
    await admin.request({ action: "create_budget", budget });
    const capture = loadJson("evals/formation/capture.json") as { events: unknown[] };
    const source = { source_id: randomUUID(), revision: "1" };
    await client.request({ action: "ingest_source", source: { reference: source, label: "Capture", scope, kind: "tool_events", owner: "test", acquired_at: new Date().toISOString(), acquisition_method: "test", precedence: null, snapshot_artifact: null }, content: { schema_version: "memory-tool-events/1", events: capture.events.slice(0, 2) } });

    const submit = async (process: "investigation" | "formation", extra: Record<string, unknown>) => {
      const task_id = randomUUID();
      const brief = { ...fixture.payload, process, task_id, scope: { ...scope, task_id }, policy: policy.policy.reference, purpose: `${process} test`,
        inputs: { memories: [], sources: [], artifacts: [] }, capabilities: { sources: [], queries: [], tools: [] }, known_conflicts: [], evidence_cutoff: new Date().toISOString(),
        profile: { id: randomUUID(), revision: 1, label: `${process} profile` },
        limits: { root_budget_id: budget.id, max_provider_attempts: 8, max_tokens: 4_000_000, max_output_bytes: 1_000_000, max_child_depth: 0, max_child_concurrency: 0 }, ...extra };
      const command = { ...fixture, request_id: randomUUID(), tenant_id, actor_id, scope: brief.scope, budget_id: budget.id, deadline: minutes(20), payload: { brief, parent_id: null, max_attempts: 2, retain_until: minutes(1440) } };
      return expectKind(await client.request({ action: "submit", command } as HostRequest), "job").job;
    };
    const investigation = await submit("investigation", {});
    const formation = await submit("formation", { inputs: { memories: [], sources: [source], artifacts: [] }, capabilities: { sources: [source], queries: [], tools: [] } });
    // A brief granting a memory revision that has since been corrected cannot be worked as written.
    const draft: RecordDraft = { label: "Stale input", scope, content: { family: "knowledge", statement: "Original", subject: null, predicate: null, uncertainty: [], examined_coverage: [] }, origin: "observed", evidential_status: "attributed_statement", availability: "routine", qualification: { status: "candidate" }, valid_time: { kind: "unknown" }, source_locators: [], derived_from: [], decision: { policy: policy.policy.reference, action: "retain", reason: "test", constraints: [], required_evidence: [], expires_at: null } };
    const contributed = expectKind(await client.request({ action: "user", request: { action: "mutate", request_id: randomUUID(), mutation: { action: "contribute", record: draft } } }), "user").result;
    if (contributed.kind !== "memory") throw new Error("memory");
    const stale = await submit("investigation", { inputs: { memories: [contributed.memory.reference], sources: [], artifacts: [] } });
    await client.request({ action: "user", request: { action: "mutate", request_id: randomUUID(), mutation: { action: "correct", expected: contributed.memory.reference, record: { ...draft, content: { family: "knowledge", statement: "Corrected", subject: null, predicate: null, uncertainty: [], examined_coverage: [] }, decision: { ...draft.decision, action: "revise" } } } } });

    // Scripted providers: the task model returns a valid WorkResult for its brief; the judgement
    // reference answers from each candidate's own evidential status.
    const faux = fauxProvider(), models = createModels();
    models.setProvider(faux.provider);
    const taskResult = { schema_version: "1", status: "complete", examined_scope: investigation.spec.brief.scope, inputs: investigation.spec.brief.inputs, findings: [], coverage: { examined: ["Scripted"], unexamined: [] }, unresolved_work: [], proposed_changes: [], child_outputs: [], known_effects: [], usage: { status: "unknown", input_tokens: null, output_tokens: null, cost: null } };
    faux.setResponses([fauxAssistantMessage(JSON.stringify(taskResult))]);
    const generative = new GenerativeProvider("reference", models, faux.getModel(), maximum, 2048, faux.getModel().id);
    let judgementCalls = 0;
    const reference: JudgementProvider = {
      id: generative.id, model: generative.model, release: generative.release, maximum, distributions: false,
      async evaluate(packet: JudgementPacket, signal: AbortSignal) {
        judgementCalls++;
        const entry = packet.evidence.find((e) => e.name === "candidate")!.content as unknown as FormationEntry;
        const choices: Record<string, string> = { "J01.assessment": entry.event.evidential_status, "J01.faithfulness": "yes", "J02.assessment": "supports", "J03.assessment": "correction", "J04.assessment": "conditional_method", "J05.assessment": "commitment", "J27.assessment": "task_local" };
        const answers = Object.fromEntries(packet.questions.flatMap((d) => d.definition.questions.map((q) => [`${d.definition.id}.${q.key}`, { type: "choice", choice: choices[`${d.definition.id}.${q.key}`] }])));
        // The task model reply is consumed first; judgement replies are queued per packet.
        faux.setResponses([fauxAssistantMessage(JSON.stringify({ answers }))]);
        return generative.evaluate(packet, signal);
      },
    };
    const events: string[] = [];
    const jobState = async (id: string) => expectKind(await client.request({ action: "inspect_job", job_id: id }), "job").job.state;
    const settled = async () => {
      const states = await Promise.all([investigation.id, formation.id, stale.id].map(jobState));
      return states.every((state) => ["completed", "partial", "failed"].includes(state));
    };
    await runSupervisor({
      host: new HostClient(url, tokens.pool), hostUrl: url, processes: ["investigation", "formation"], concurrency: { interactive: 1, deferred: 1 }, pollMs: 50, leaseSeconds: 60,
      sessionsDirectory: join(base, "sessions"), models, taskModel: faux.getModel(), maxPayloadBytes: 100_000,
      judgement: { reference, mode: "reference", maximum },
      log: (line) => events.push(line),
      until: settled,
    }, context);

    const doneInvestigation = expectKind(await client.request({ action: "inspect_job", job_id: investigation.id }), "job").job;
    expect(doneInvestigation.state).toBe("completed");
    expect(doneInvestigation.result?.coverage.examined).toEqual(["Scripted"]);
    const doneFormation = expectKind(await client.request({ action: "inspect_job", job_id: formation.id }), "job").job;
    // Formation reports material it did not examine; a partial result must then say what remains.
    expect(["completed", "partial"]).toContain(doneFormation.state);
    if (doneFormation.state === "partial") expect(doneFormation.result?.unresolved_work.length).toBeGreaterThan(0);
    expect(judgementCalls).toBeGreaterThan(0);
    const records = expectKind(await client.request({ action: "user", request: { action: "browse", query: { include_inactive: false, limit: 50 } } }), "user").result;
    if (records.kind !== "records") throw new Error("records");
    // Two formation candidates plus the contributed (later corrected) input memory.
    expect(records.records.length).toBe(3);
    expect(events.some((e) => e.includes("claimed") && e.includes(investigation.id))).toBe(true);
    expect(events.some((e) => e.includes("claimed") && e.includes(formation.id))).toBe(true);
    const blocked = expectKind(await client.request({ action: "inspect_job", job_id: stale.id }), "job").job;
    expect(blocked.state).toBe("partial");
    expect(blocked.result?.status).toBe("blocked");
    expect(blocked.result?.unresolved_work.join(" ")).toMatch(/revision 2/);
    expect(blocked.attempt).toBe(1);
    // Each job was claimed exactly once; the loop stopped as soon as all settled.
    expect(events.filter((e) => e.includes("claimed"))).toHaveLength(3);
  } finally {
    host.kill("SIGINT");
    await once(host, "exit");
    await rm(base, { recursive: true, force: true });
  }
}, 120_000);

it("fault: a worker that dies after claiming loses its lease; recovery requeues and a second attempt completes", async () => {
  const w = await world();
  try {
    const job = await w.submit("investigation", {});
    // A worker that claims with a short lease and then dies: no start, no renewal, no result.
    const claimed = await w.pool.request({ action: "claim_next", processes: ["investigation"], lease_seconds: 2 });
    expect(claimed.kind).toBe("work");
    expect((await w.job(job.id)).state).toBe("leased");
    // While the lease lives nobody else can claim it; after expiry the Host's recovery timer requeues it.
    expect((await w.pool.request({ action: "claim_next", processes: ["investigation"], lease_seconds: 60 })).kind).toBe("idle");
    const started = Date.now();
    while ((await w.job(job.id)).state !== "queued") {
      if (Date.now() - started > 30_000) throw new Error("recovery did not requeue the job");
      await new Promise((d) => setTimeout(d, 500));
    }
    expect((await w.job(job.id)).attempt).toBe(1);
    const faux = fauxProvider(), models = createModels();
    models.setProvider(faux.provider);
    const result = { schema_version: "1", status: "complete", examined_scope: job.spec.brief.scope, inputs: job.spec.brief.inputs, findings: [], coverage: { examined: ["Second attempt"], unexamined: [] }, unresolved_work: [], proposed_changes: [], child_outputs: [], known_effects: [], usage: { status: "unknown", input_tokens: null, output_tokens: null, cost: null } };
    faux.setResponses([fauxAssistantMessage(JSON.stringify(result))]);
    await runSupervisor({
      host: w.pool, hostUrl: w.url, processes: ["investigation"], concurrency: 1, pollMs: 50, leaseSeconds: 60, sessionsDirectory: join(w.base, "sessions"),
      models, taskModel: faux.getModel(), maxPayloadBytes: 100_000, judgement: { reference: scriptedReference(faux, models), mode: "reference", maximum },
      log: () => {}, until: async () => SETTLED.includes((await w.job(job.id)).state),
    }, context);
    const done = await w.job(job.id);
    expect(done.state).toBe("completed");
    expect(done.attempt).toBe(2);
    expect(done.result?.coverage.examined).toEqual(["Second attempt"]);
  } finally {
    await w.close();
  }
}, 120_000);

it("fault: a judgement provider outage defers the candidates with a recorded reason; a later job retains them", async () => {
  const w = await world();
  try {
    const source = await w.ingestCapture();
    const formationJob = (extra = {}) => w.submit("formation", { inputs: { memories: [], sources: [source], artifacts: [] }, capabilities: { sources: [source], queries: [], tools: [] }, ...extra });
    const faux = fauxProvider(), models = createModels();
    models.setProvider(faux.provider);
    let outage = true, calls = 0;
    const reference = scriptedReference(faux, models, () => { calls++; if (outage) throw new Error("provider unavailable"); });
    const run = async (jobId: string) => runSupervisor({
      host: w.pool, hostUrl: w.url, processes: ["formation"], concurrency: 1, pollMs: 50, leaseSeconds: 60, sessionsDirectory: join(w.base, "sessions"),
      models, taskModel: faux.getModel(), maxPayloadBytes: 100_000, judgement: { reference, mode: "reference", maximum }, log: () => {},
      until: async () => SETTLED.includes((await w.job(jobId)).state),
    }, context);
    const first = await formationJob();
    await run(first.id);
    const failed = await w.job(first.id);
    // The outage is recorded as unavailable assessments; nothing is retained and the result says why.
    expect(failed.state).toBe("partial");
    expect(failed.attempt).toBe(1);
    expect(calls).toBeGreaterThan(0);
    expect(failed.result?.unresolved_work.join(" ")).toMatch(/defer|unavailable|unresolved/i);
    const browse = expectKind(await w.client.request({ action: "user", request: { action: "browse", query: { include_inactive: false, limit: 50 } } }), "user").result;
    expect(browse.kind === "records" ? browse.records.length : -1).toBe(0);
    // Provider restored. The same purpose would continue the source cursor past the deferred
    // events; a new purpose explicitly reinterprets the source, and the candidates are retained.
    outage = false;
    const second = await formationJob({ purpose: "formation retry after provider outage" });
    await run(second.id);
    expect(["completed", "partial"]).toContain((await w.job(second.id)).state);
    const after = expectKind(await w.client.request({ action: "user", request: { action: "browse", query: { include_inactive: false, limit: 50 } } }), "user").result;
    expect(after.kind === "records" ? after.records.length : -1).toBe(2);
  } finally {
    await w.close();
  }
}, 120_000);
