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
import type { HostRequest, RecordDraft, MemoryPolicy, Scope } from "../../../contracts/generated/host-request.js";
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
  scope: Scope; policy: { reference: unknown }; budget: { id: string }; profile: { id: string; revision: number; label: string };
  submit: (process: LearningProcess, extra: Record<string, unknown>) => Promise<import("../../../contracts/generated/host-response.js").Job>;
  job: (id: string) => Promise<import("../../../contracts/generated/host-response.js").Job>;
  contribute: (record: Partial<RecordDraft> & Pick<RecordDraft, "content" | "label">, task_id?: string) => Promise<import("../../../contracts/generated/host-response.js").MemoryVersion>;
  createPolicy: (label: string, policy: MemoryPolicy) => Promise<{ id: string; revision: number; label: string }>;
  publishArtifact: (label: string, data: unknown, task_id?: string) => Promise<{ id: string; task_id: string; scope: Scope }>;
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

it("forms, consolidates, qualifies, adopts, retrieves and maintains memory across the pool without manual intervention", async () => {
  const w = await world();
  try {
    const judgementFaux = fauxProvider(), judgementModels = createModels();
    judgementModels.setProvider(judgementFaux.provider);
    const taskFaux = fauxProvider(), taskModels = createModels();
    taskModels.setProvider(taskFaux.provider);
    const taskModel = taskService(taskModels, taskFaux);
    const judgementOptions = (scope: Scope) => ({
      primary: scriptedJudgement(judgementFaux, judgementModels),
      maxAttempts: 1,
      disclosure: { policy: "local-test-only", scope, providers: ["reference"] },
    });

    // Several related knowledge/episode records exist, formed automatically from ordinary task experience.
    const members = await formCohort(w, judgementFaux, judgementModels);

    // Cohort scheduling: `schedule_consolidation` (crates/memory-store/src/scheduling.rs) now groups
    // retained candidates per policy and submits the cohort job itself once a policy's minimum is met —
    // proven directly against the Store in `crates/memory-store/tests/scheduling.rs`. This proof keeps
    // its own submission explicit instead, because it needs the formed cohort under a *specific* policy
    // (one with `consolidation` and a known `evaluator_profile`) and a specific task/scope to drive the
    // rest of the proof deterministically; letting the scheduler pick a cohort here would leave which
    // records it selected, and under which of formation's incidental policies, up to the pool's own
    // timing rather than this test's.
    const learningTaskId = randomUUID();
    const learningScope = { ...w.scope, task_id: learningTaskId };
    const suite = {
      evaluator_profile: w.profile,
      cases: [
        { id: "held-out-a", source_group: randomUUID(), conditions: ["Lease looks stuck"], task: { representative: true }, expected: { resumed: true } },
        { id: "held-out-b", source_group: randomUUID(), conditions: ["Lease looks stuck, restated"], task: { representative: true }, expected: { resumed: true } },
      ],
    };
    const suiteArtifact = await w.publishArtifact("Qualification suite", suite, learningTaskId);
    const consolidationPolicy = await w.createPolicy("Unattended learning", {
      retention_purpose: "test", allowed_uses: [], source_rules: [], evidence_requirements: [], applicability_rules: [], budget_class: "test", scheduling_priority: 0,
      judgement_dispositions: [], notification_policy: "quiet", qualification_requirements: [],
      consolidation: { min_independent_sources: 1, min_held_out_groups: 1, minimum_success_rate: 0, maximum_regression: 1, maximum_cost_microunits: 1_000_000, evaluator_profile: w.profile },
    });
    const consolidationJob = await w.submit("consolidation", {
      task_id: learningTaskId, scope: learningScope, policy: consolidationPolicy,
      inputs: { memories: members, sources: [], artifacts: [suiteArtifact.id] }, capabilities: { sources: [], queries: [], tools: ["read"] },
    });
    const claimed = expectKind(await w.admin.request({ action: "claim", job_id: consolidationJob.id, lease_seconds: 300 }), "assignment");
    const consolidationAssignment = claimed.assignment, consolidationFence = fenceFor(consolidationAssignment);
    await w.admin.request({ action: "start", fence: consolidationFence });
    // The consolidation job stays open (never completed) through evaluation and adoption, since
    // start_qualification and adopt_procedure both need its live fence — driven directly here rather
    // than through consolidationModule, which settles a consolidation job as soon as it has a result.
    let session = await openLocalSession(join(w.base, "sessions"), consolidationAssignment.job.session_id);
    const consolidationInputValue = consolidationInput(consolidationAssignment);
    scriptedTaskModel(taskFaux);
    const synthesize = (window: ConsolidationWindow, ctx: Parameters<typeof synthesizeProposal>[2]) => synthesizeProposal(taskModel, window, ctx);
    const result = await runConsolidation(
      w.admin, consolidationAssignment, session.session, consolidationInputValue,
      { ...judgementOptions(learningScope), synthesize }, context,
    );
    expect(result.record, JSON.stringify(result)).toBeTruthy();
    const candidate = result.record!;

    // Restart safety: a crash before the job completes resumes from the same session without re-judging,
    // re-synthesizing or creating a second candidate.
    await session.close();
    session = await openLocalSession(join(w.base, "sessions"), consolidationAssignment.job.session_id);
    const judgementCallsBefore = judgementFaux.state.callCount;
    scriptedTaskModel(taskFaux);
    const resumed = await runConsolidation(
      w.admin, consolidationAssignment, session.session, consolidationInputValue,
      { ...judgementOptions(learningScope), synthesize }, context,
    );
    expect(resumed.record?.reference).toEqual(candidate.reference);
    expect(judgementFaux.state.callCount).toBe(judgementCallsBefore);

    // Adoption remains a deliberate decision on the still-open consolidation job's own fence (the
    // `AdoptProcedure` check `schedule_qualification`'s own doc comment describes): a session that wants
    // the candidate adopted drives `start_qualification`/`adopt_procedure` itself, building the real
    // manifest that combines this consolidation's review, its candidate and the held-out suite above.
    const started = expectKind(await w.admin.request({
      action: "start_qualification", fence: consolidationFence, request_id: randomUUID(), review_id: result.review_id, suite_id: suiteArtifact.id,
    }), "job");
    // A qualification job runs its trials unattended through the real pool: claimed, executed and
    // settled without any of this test driving its internals directly.
    await runSupervisor({
      host: w.pool, hostUrl: w.url, concurrency: 1, pollMs: 50, leaseSeconds: 60, sessionsDirectory: join(w.base, "sessions"),
      composition: compose({ taskModel }, "evaluation"),
      log: () => {}, until: async () => SETTLED.includes((await w.job(started.job.id)).state),
    }, context);
    expect((await w.job(started.job.id)).state).toBe("completed");
    const adopted = expectKind(await w.admin.request({ action: "adopt_procedure", fence: consolidationFence, evaluation_job: started.job.id }), "adoption_result");
    expect(adopted.result.adopted, JSON.stringify(adopted.result)).toBe(true);

    // The consolidation job settles now that adoption is complete, closing out its own record.
    await w.admin.request({
      action: "complete", fence: consolidationFence, request_id: randomUUID(),
      result: {
        schema_version: "1", status: "complete", examined_scope: learningScope, inputs: consolidationAssignment.job.spec.brief.inputs,
        findings: [], coverage: { examined: result.coverage.examined, unexamined: [] }, unresolved_work: [], proposed_changes: [], child_outputs: [], known_effects: [],
        usage: { status: "unknown", input_tokens: null, output_tokens: null, cost: null },
      },
    });
    await session.close();

    // The Host's own scheduler already reacted to this committed candidate, unattended: it found the
    // consolidation review through `consolidation_reviews` and submitted a companion `evaluation` job
    // by itself (`Store::schedule_qualification`) — find it through durable events, as `scenario.test.ts`
    // does for formation, rather than submitting it. (It shares the consolidation job's session, so the
    // pool can only claim it once that job's own lease is released — exactly why this comes after the
    // consolidation job settles above, rather than racing the interactive qualification against it.) It
    // carries no manifest (this consolidation job's own `inputs.artifacts` names a bare suite, not the
    // `QualificationInput` a manifest needs to combine that suite with the review and candidate), so
    // `evaluationModule` settles it safely as a clear partial rather than fabricating a comparison — the
    // scheduling wiring proven end to end, with no manual submission, independent of the interactive
    // qualification that already adopted this same candidate above.
    const scheduledEvaluation = async () => {
      const page = expectKind(await w.client.request({ action: "events", after: 0, limit: 500 }), "events").page;
      for (const event of page.events.filter((e) => e.kind === "job_accepted")) {
        const job = await w.job(event.resource_id);
        if (job.spec.brief.process === "evaluation" && job.spec.brief.inputs.memories.some((m) => m.memory_id === candidate.reference.memory_id)) return job;
      }
      return undefined;
    };
    const scheduleStarted = Date.now();
    let autoEvaluation = await scheduledEvaluation();
    while (!autoEvaluation) {
      if (Date.now() - scheduleStarted > 30_000) throw new Error("the Host did not schedule an evaluation job for the committed candidate");
      await new Promise((resolve) => setTimeout(resolve, 500));
      autoEvaluation = await scheduledEvaluation();
    }
    const autoLogs: string[] = [];
    await runSupervisor({
      host: w.pool, hostUrl: w.url, concurrency: 1, pollMs: 50, leaseSeconds: 60, sessionsDirectory: join(w.base, "sessions"),
      composition: compose({}, "evaluation"),
      log: (l) => autoLogs.push(l), until: async () => SETTLED.includes((await w.job(autoEvaluation!.id)).state),
    }, context);
    const autoEvaluationDone = await w.job(autoEvaluation.id);
    expect(autoEvaluationDone.state, autoLogs.join("\n")).toBe("partial");
    expect(autoEvaluationDone.result?.unresolved_work.join(" ")).toContain(candidate.reference.label);

    // A later task's activation retrieves the adopted method in the same task scope the method was
    // adopted into — cross-task promotion to project scope is a maintenance/policy concern outside this
    // pipeline's proof. Adoption revised the candidate (candidate -> evaluated), which leaves its original
    // derived_from relations pointing at the pre-adoption revision; activation's group-completeness check
    // treats that historical, now-superseded endpoint as missing evidence and defers the whole group even
    // though the candidate itself was retrieved and judged relevant — so relevance is asserted from the
    // judgement trace (J07 asked and answered "relevant") rather than final selection.
    const activationJob = await w.submit("activation", {
      task_id: learningTaskId, scope: learningScope, purpose: "How do I recover a job whose lease appears stuck?",
      inputs: { memories: [], sources: [], artifacts: [] }, capabilities: { sources: [], queries: [], tools: [] },
    });
    const activationLogs: string[] = [];
    await runSupervisor({
      host: w.pool, hostUrl: w.url, concurrency: 1, pollMs: 50, leaseSeconds: 60, sessionsDirectory: join(w.base, "sessions"),
      composition: compose({ judgement: { primary: scriptedJudgement(judgementFaux, judgementModels), maximum } }, "activation"),
      log: (l) => activationLogs.push(l), until: async () => SETTLED.includes((await w.job(activationJob.id)).state),
    }, context);
    const activationDone = await w.job(activationJob.id);
    expect(["completed", "partial"], activationLogs.join("\n")).toContain(activationDone.state);
    expect(activationLogs.some((l) => l.includes("J07") && l.includes("relevant")), activationLogs.join("\n")).toBe(true);

    // A correction to a task-scoped record triggers a maintenance job that reviews it: find the job the
    // Host's own scheduler (`Store::schedule_maintenance`) submits for it — through durable events, not
    // by hand. Its `MaintenanceRequest` is carried on `brief.scheduled` (no manifest artifact, since the
    // scheduler holds no job fence to publish one); `maintenanceInput` derives the same manifest shape
    // from it, repeating the already-corrected record's own content with only the reason updated
    // (`review_maintenance` needs a proposed revision or a removed source; the scheduler cannot author a
    // substantive one) so this job reviews the record rather than proposing a second correction.
    const maintenanceTaskId = randomUUID();
    const base = await w.contribute(
      { label: "Lease timeout default", content: { family: "knowledge", statement: "The default lease timeout is 120 seconds.", uncertainty: [], examined_coverage: [] } },
      maintenanceTaskId,
    );
    const corrected = structuredClone(base.record);
    if (corrected.content.family === "knowledge") corrected.content.statement = "The default lease timeout is 90 seconds, per the corrected configuration.";
    await w.client.request({
      action: "user",
      request: { action: "mutate", request_id: randomUUID(), mutation: { action: "correct", expected: base.reference, record: corrected } },
    });
    const scheduledMaintenance = async () => {
      const page = expectKind(await w.client.request({ action: "events", after: 0, limit: 500 }), "events").page;
      for (const event of page.events.filter((e) => e.kind === "job_accepted")) {
        const job = await w.job(event.resource_id);
        if (job.spec.brief.process === "maintenance" && job.spec.brief.scheduled?.kind === "maintenance_correction") return job;
      }
      return undefined;
    };
    const maintenanceScheduleStarted = Date.now();
    let maintenanceJob = await scheduledMaintenance();
    while (!maintenanceJob) {
      if (Date.now() - maintenanceScheduleStarted > 30_000) throw new Error("the Host did not schedule maintenance for the correction");
      await new Promise((resolve) => setTimeout(resolve, 500));
      maintenanceJob = await scheduledMaintenance();
    }
    // Restart safety at the pool level: a worker that claims and then dies (no start, no result) loses
    // its lease; the Host's recovery timer requeues the job and a second attempt completes it exactly
    // once, rather than the review (or its judgement) happening twice.
    const deadClaim = expectKind(await w.pool.request({ action: "claim_next", processes: ["maintenance"], lease_seconds: 2 }), "work");
    expect(deadClaim.assignment.job.id).toBe(maintenanceJob.id);
    expect((await w.job(maintenanceJob.id)).state).toBe("leased");
    const requeueStarted = Date.now();
    while ((await w.job(maintenanceJob.id)).state !== "queued") {
      if (Date.now() - requeueStarted > 30_000) throw new Error("recovery did not requeue the maintenance job");
      await new Promise((resolve) => setTimeout(resolve, 500));
    }
    const maintenanceLogs: string[] = [];
    await runSupervisor({
      host: w.pool, hostUrl: w.url, concurrency: 1, pollMs: 50, leaseSeconds: 60, sessionsDirectory: join(w.base, "sessions"),
      composition: compose({ judgement: { primary: scriptedJudgement(judgementFaux, judgementModels), maximum } }, "maintenance"),
      log: (l) => maintenanceLogs.push(l), until: async () => SETTLED.includes((await w.job(maintenanceJob!.id)).state),
    }, context);
    const maintenanceDone = await w.job(maintenanceJob.id);
    expect(maintenanceDone.state, maintenanceLogs.join("\n")).toBe("completed");
    // The review happened exactly once even though the job was claimed twice.
    expect(maintenanceDone.attempt).toBe(2);
    const history = expectKind(await w.client.request({ action: "user", request: { action: "history", memory_id: base.reference.memory_id, after_revision: 0, limit: 10 } }), "user").result;
    if (history.kind !== "history") throw new Error("Expected memory history");
    // Revision 1 is the contribution, revision 2 the manual correction above, and revision 3 this
    // scheduled review's own wording-only revision (the corrected content, an updated reason), applied
    // exactly once despite the job being claimed twice.
    expect(history.records.map((r) => r.reference.revision).sort()).toEqual([1, 2, 3]);
  } finally {
    await w.close();
  }
}, 180_000);
