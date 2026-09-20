// ABOUTME: End-to-end pool coverage for the learning processes: one settled job of consolidation, evaluation,
// ABOUTME: maintenance and activation each through a real Host, plus the full unattended learning pipeline.
import { spawn } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { once } from "node:events";
import { createServer } from "node:net";
import { randomBytes, randomUUID } from "node:crypto";
import { BACKGROUND_CONTEXT as context } from "@earendil-works/pi-agent-core";
import { createModels, fauxProvider, fauxAssistantMessage, type TranscriptContext } from "@earendil-works/pi-ai";
import { expect, it } from "vitest";
import type { HostRequest, RecordDraft, MemoryPolicy } from "../../../contracts/generated/host-request.js";
import type { HostResponse } from "../../../contracts/generated/host-response.js";
import type { JudgementPacket } from "../../../contracts/generated/judgement-packet.js";
import type { ConsolidationWindow } from "../../../contracts/generated/consolidation-review.js";
import type { FormationEntry } from "../../../contracts/generated/formation-window.js";
import type { MaintenanceManifest } from "../src/maintenance.js";
import { HostClient } from "../../host-client/src/client.js";
import { fenceFor } from "../../host-client/src/fence.js";
import { openLocalSession } from "../../pi-worker/src/local-session.js";
import { input, loadJson } from "../../pi-worker/test/fixtures.js";
import { GenerativeProvider, type JudgementProvider } from "../../judgement/src/providers.js";
import { maximum } from "../../judgement/test/fixtures.js";
import { runConsolidation } from "../../consolidation/src/index.js";
import { POOL_MODULES, composeApplication, runSupervisor, type Services } from "../src/index.js";
import { consolidationInput } from "../src/consolidation.js";
import { synthesizeProposal } from "../src/adapters/synthesis.js";

type LearningProcess = "investigation" | "formation" | "consolidation" | "evaluation" | "maintenance" | "activation";

/** Composes the pool's modules for the named processes against scripted services. */
function compose(services: Services, ...processes: LearningProcess[]) {
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

interface World {
  base: string; url: string; host: ReturnType<typeof spawn>; client: HostClient; admin: HostClient; pool: HostClient;
  scope: Record<string, unknown>; policy: { reference: unknown }; budget: { id: string }; profile: { id: string; revision: number; label: string };
  submit: (process: LearningProcess, extra: Record<string, unknown>) => Promise<import("../../../contracts/generated/host-response.js").Job>;
  job: (id: string) => Promise<import("../../../contracts/generated/host-response.js").Job>;
  contribute: (record: Partial<RecordDraft> & Pick<RecordDraft, "content" | "label">, task_id?: string) => Promise<import("../../../contracts/generated/host-response.js").MemoryVersion>;
  createPolicy: (label: string, policy: MemoryPolicy) => Promise<{ id: string; revision: number; label: string }>;
  publishArtifact: (label: string, data: unknown, task_id?: string) => Promise<{ id: string; task_id: string; scope: Record<string, unknown> }>;
  ingestCapture: (events: unknown[]) => Promise<{ source_id: string; revision: string }>;
  close: () => Promise<void>;
}
/** Own copy for this test file: `packages/supervisor/test/supervisor.test.ts` must not be imported or edited. */
async function world(): Promise<World> {
  const base = await mkdtemp(join(tmpdir(), "memory-learning-"));
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
    action: "create_policy", label: "Learning test", scope, effective: { kind: "unknown" },
    policy: { retention_purpose: "test", allowed_uses: ["task_context"], source_rules: [], evidence_requirements: [], applicability_rules: [], budget_class: "test", scheduling_priority: 0, judgement_dispositions: [], notification_policy: "material", qualification_requirements: [] } } } }), "user").result;
  if (created.kind !== "policy") throw new Error("policy");
  const budget = { id: randomUUID(), scope, limit: { tokens: 5_000_000, provider_calls: 200, output_bytes: 5_000_000, cost_microunits: 5_000_000, sandbox_cpu_ms: 0, sandbox_time_ms: 0 }, final_result_reserve: { tokens: 100, provider_calls: 0, output_bytes: 1000, cost_microunits: 0, sandbox_cpu_ms: 0, sandbox_time_ms: 0 }, deadline: minutes(30), max_child_depth: 1, max_child_concurrency: 2, pricing_revision: "test" };
  await admin.request({ action: "create_budget", budget });
  const profile = { id: randomUUID(), revision: 1, label: "learning-profile" };
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
    base, url, host, client, admin, pool, scope, policy: { reference: created.policy.reference }, budget, profile,
    submit,
    async job(id) { return expectKind(await client.request({ action: "inspect_job", job_id: id }), "job").job; },
    async contribute(record, task_id) {
      const draft: RecordDraft = {
        scope: task_id ? { ...scope, task_id } : scope, origin: "observed", evidential_status: "attributed_statement", availability: "routine", qualification: { status: "candidate" },
        valid_time: { kind: "unknown" }, source_locators: [], derived_from: [],
        decision: { policy: created.policy.reference, action: "retain", reason: "Contributed for the learning proof", constraints: [], required_evidence: [], expires_at: null },
        ...record,
      };
      const result = expectKind(await client.request({ action: "user", request: { action: "mutate", request_id: randomUUID(), mutation: { action: "contribute", record: draft } } }), "user").result;
      if (result.kind !== "memory") throw new Error(`Expected a contributed memory, got ${result.kind}`);
      return result.memory;
    },
    async createPolicy(label, policy) {
      const result = expectKind(await admin.request({ action: "user", request: { action: "mutate", request_id: randomUUID(), mutation: { action: "create_policy", label, scope, effective: { kind: "unknown" }, policy } } }), "user").result;
      if (result.kind !== "policy") throw new Error(`Expected a created policy, got ${result.kind}`);
      return result.policy.reference;
    },
    async publishArtifact(label, data, existingTaskId) {
      // A holder job supplies the fence `publish_artifact` needs; a job's own scope is always narrowed to
      // its own task (the Host rejects a task-agnostic brief), so the artifact this produces is only
      // readable by a later job that shares this same task_id and scope. The holder is completed right
      // away (its session and lease are exclusive to it) so the real job below can claim that same task.
      const task_id = existingTaskId ?? randomUUID();
      const holderScope = { ...scope, task_id };
      const holder = await submit("investigation", { task_id, scope: holderScope });
      const claimed = expectKind(await admin.request({ action: "claim", job_id: holder.id, lease_seconds: 300 }), "assignment");
      const fence = fenceFor(claimed.assignment);
      await admin.request({ action: "start", fence });
      const published = expectKind(await admin.request({ action: "publish_artifact", fence, request_id: randomUUID(), label, text: JSON.stringify(data), dependencies: [] }), "artifact");
      await admin.request({
        action: "complete", fence, request_id: randomUUID(),
        result: { schema_version: "1", status: "complete", examined_scope: holderScope, inputs: { memories: [], sources: [], artifacts: [published.artifact.id] }, findings: [], coverage: { examined: [label], unexamined: [] }, unresolved_work: [], proposed_changes: [], child_outputs: [published.artifact.id], known_effects: [], usage: { status: "unknown", input_tokens: null, output_tokens: null, cost: null } },
      });
      return { id: published.artifact.id, task_id, scope: holderScope };
    },
    async ingestCapture(events) {
      const source = { source_id: randomUUID(), revision: "1" };
      await client.request({ action: "ingest_source", source: { reference: source, label: "Capture", scope, kind: "tool_events", owner: "test", acquired_at: new Date().toISOString(), acquisition_method: "test", precedence: null, snapshot_artifact: null }, content: { schema_version: "memory-tool-events/1", events } });
      return source;
    },
    async close() { host.kill("SIGINT"); await once(host, "exit"); await rm(base, { recursive: true, force: true }); },
  };
}

/** Every judgement family this file's jobs ask, answered deterministically from the packet's own evidence
 * so the same scripted provider serves consolidation, evaluation adoption, maintenance and activation. */
function packetChoice(packet: JudgementPacket, key: string): string {
  if (key === "J11.comparable") return "yes";
  if (key === "J11.counterexample" || key === "J11.duplicate") return "no";
  if (key.startsWith("J12.")) return "yes";
  if (key === "J13.assessment") return "correction";
  if (key === "J14.assessment") return "compatible";
  if (key === "J15.assessment") return "none";
  if (key === "J16.assessment") return "unaffected";
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
  throw new Error(`Learning test has no scripted answer for ${key}; packet: ${JSON.stringify(packet.questions.map((q) => q.definition.id))}`);
}
/** Scripts each call as it happens rather than pre-queueing a fixed count, since the calls a job makes
 * depend on its own content. */
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

/** A bounded, JSON-answering task model reply, scripted per call from the request payload it receives so
 * both the synthesis adapter (one call) and the qualification trial adapter (one call per case/arm) can
 * share the same faux model across a whole scenario. */
function scriptedTaskModel(faux: ReturnType<typeof fauxProvider>) {
  const textOf = (content: string | { type: string; text?: string }[]) => (typeof content === "string" ? content : content.map((c) => c.text ?? "").join(""));
  const reply = (t: TranscriptContext) => {
    const last = t.messages.at(-1);
    const request = JSON.parse(textOf((last as { content: string | { type: string; text?: string }[] }).content)) as Record<string, unknown>;
    if ("cases" in request)
      return fauxAssistantMessage(JSON.stringify({
        label: "Restart recovery method", concise_summary: "Restart the worker pool to clear a stuck lease.",
        invariant: "A stuck lease clears when the pool restarts", variations: [], untested: [],
        clause_text: "Restart the worker pool when a job's lease appears stuck", conditions: ["Lease looks stuck"], uncertainty: [],
        support_case_ids: (request.cases as { id: string }[]).map((c) => c.id),
        is_procedure: true, purpose: "Recover a stuck job lease", applicability: ["A job's lease has not renewed"], exclusions: ["No lease is stuck"],
        steps: ["Restart the worker pool", "Reclaim the job"], evidence_criteria: ["The job resumes after restart"],
        decision_points: ["Has the lease expired without renewal?"], stopping_criteria: ["The job resumes, or the lease is confirmed lost"], checks: ["The job's attempt count increased"],
      }));
    return fauxAssistantMessage(JSON.stringify({ answer: JSON.stringify({ resumed: true }), trajectory: ["Applied the representation to the task"] }));
  };
  // One queued factory per call this scenario might make; each invocation reads its own request fresh.
  faux.setResponses(Array.from({ length: 12 }, () => reply));
}

/** Ingests one experience source and runs formation over it through the pool, returning the retained
 * records so a later consolidation can build a genuinely source-attributed cohort from them. */
async function formCohort(w: World, judgementFaux: ReturnType<typeof fauxProvider>, judgementModels: ReturnType<typeof createModels>) {
  const capture = loadJson("evals/formation/capture.json") as { events: unknown[] };
  const source = await w.ingestCapture(capture.events.slice(0, 2));
  const job = await w.submit("formation", { inputs: { memories: [], sources: [source], artifacts: [] }, capabilities: { sources: [source], queries: [], tools: [] } });
  await runSupervisor({
    host: w.pool, hostUrl: w.url, concurrency: 1, pollMs: 50, leaseSeconds: 60, sessionsDirectory: join(w.base, "sessions"),
    composition: compose({ judgement: { primary: scriptedJudgement(judgementFaux, judgementModels), maximum } }, "formation"),
    log: () => {}, until: async () => SETTLED.includes((await w.job(job.id)).state),
  }, context);
  const done = await w.job(job.id);
  if (!["completed", "partial"].includes(done.state)) throw new Error(`Formation did not settle: ${done.state}`);
  const browse = expectKind(await w.client.request({ action: "user", request: { action: "browse", query: { include_inactive: false, limit: 50 } } }), "user").result;
  if (browse.kind !== "records") throw new Error("Expected browsed records");
  if (!browse.records.length) throw new Error("Formation retained no records to consolidate");
  return browse.records.map((r) => r.reference);
}

it("a consolidation job settles through the pool with a coherent result", async () => {
  const w = await world();
  try {
    const judgementFaux = fauxProvider(), judgementModels = createModels();
    judgementModels.setProvider(judgementFaux.provider);
    const members = await formCohort(w, judgementFaux, judgementModels);
    const consolidation = await w.createPolicy("Consolidation settles", {
      retention_purpose: "test", allowed_uses: [], source_rules: [], evidence_requirements: [], applicability_rules: [], budget_class: "test", scheduling_priority: 0,
      judgement_dispositions: [], notification_policy: "quiet", qualification_requirements: [],
      consolidation: { min_independent_sources: 1, min_held_out_groups: 1, minimum_success_rate: 0, maximum_regression: 1, maximum_cost_microunits: 1_000_000, evaluator_profile: w.profile },
    });
    const job = await w.submit("consolidation", { policy: consolidation, inputs: { memories: members, sources: [], artifacts: [] }, capabilities: { sources: [], queries: [], tools: ["read"] } });
    const taskFaux = fauxProvider(), taskModels = createModels();
    taskModels.setProvider(taskFaux.provider);
    scriptedTaskModel(taskFaux);
    const logs: string[] = [];
    await runSupervisor({
      host: w.pool, hostUrl: w.url, concurrency: 1, pollMs: 50, leaseSeconds: 60, sessionsDirectory: join(w.base, "sessions"),
      composition: compose({ judgement: { primary: scriptedJudgement(judgementFaux, judgementModels), maximum }, taskModel: taskService(taskModels, taskFaux) }, "consolidation"),
      log: (l) => logs.push(l), until: async () => SETTLED.includes((await w.job(job.id)).state),
    }, context);
    const done = await w.job(job.id);
    expect(["completed", "partial"], logs.join("\n")).toContain(done.state);
    expect(done.result?.coverage.examined.length).toBeGreaterThan(0);
  } finally {
    await w.close();
  }
}, 60_000);

it("a maintenance job settles a correction through the pool", async () => {
  const w = await world();
  try {
    const taskId = randomUUID();
    const before = await w.contribute({ label: "Beam clearance", content: { family: "knowledge", statement: "Clearance under beam B3 is 2.0 m.", uncertainty: [], examined_coverage: [] } }, taskId);
    const after = structuredClone(before.record);
    after.label = "Corrected beam clearance";
    if (after.content.family === "knowledge") after.content.statement = "Clearance under beam B3 is 2.4 m, per the corrected survey.";
    const manifest: MaintenanceManifest = { kind: "correction", id: randomUUID(), request: { before: before.reference, after, removed_sources: [], candidates: [], reason: "Corrected survey reading" } };
    const manifestArtifact = await w.publishArtifact("Maintenance manifest", manifest, taskId);
    const logs: string[] = [];
    const job = await w.submit("maintenance", { task_id: manifestArtifact.task_id, scope: manifestArtifact.scope, inputs: { memories: [before.reference], sources: [], artifacts: [manifestArtifact.id] }, capabilities: { sources: [], queries: [], tools: [] } });
    const judgementFaux = fauxProvider(), judgementModels = createModels();
    judgementModels.setProvider(judgementFaux.provider);
    await runSupervisor({
      host: w.pool, hostUrl: w.url, concurrency: 1, pollMs: 50, leaseSeconds: 60, sessionsDirectory: join(w.base, "sessions"),
      composition: compose({ judgement: { primary: scriptedJudgement(judgementFaux, judgementModels), maximum } }, "maintenance"),
      log: (l) => logs.push(l), until: async () => SETTLED.includes((await w.job(job.id)).state),
    }, context);
    const done = await w.job(job.id);
    expect(done.state, logs.join("\n")).toBe("completed");
    expect(done.result?.child_outputs.length).toBe(1);
  } finally {
    await w.close();
  }
}, 60_000);

it("an activation job settles a standalone retrieval through the pool", async () => {
  const w = await world();
  try {
    const memory = await w.contribute({ label: "Beam clearance", content: { family: "knowledge", statement: "Clearance under beam B3 is 2.0 m.", uncertainty: [], examined_coverage: [] } });
    const logs: string[] = [];
    const job = await w.submit("activation", { purpose: "What clearance is available under beam B3?", inputs: { memories: [], sources: [], artifacts: [] }, capabilities: { sources: [], queries: [], tools: [] } });
    const judgementFaux = fauxProvider(), judgementModels = createModels();
    judgementModels.setProvider(judgementFaux.provider);
    await runSupervisor({
      host: w.pool, hostUrl: w.url, concurrency: 1, pollMs: 50, leaseSeconds: 60, sessionsDirectory: join(w.base, "sessions"),
      composition: compose({ judgement: { primary: scriptedJudgement(judgementFaux, judgementModels), maximum } }, "activation"),
      log: (l) => logs.push(l), until: async () => SETTLED.includes((await w.job(job.id)).state),
    }, context);
    const done = await w.job(job.id);
    expect(["completed", "partial"]).toContain(done.state);
    expect(done.result?.child_outputs.length).toBe(1);
    expect(done.result?.coverage.examined.some((line) => line.includes(memory.reference.label)), logs.join("\n")).toBe(true);
  } finally {
    await w.close();
  }
}, 60_000);
