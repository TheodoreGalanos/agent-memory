// ABOUTME: Headless end-to-end scenario against a real Host: a task's real tool use becomes retained
// ABOUTME: knowledge through automatic formation, then reaches a related task in its own project only.
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
import type { JudgementPacket } from "../../../contracts/generated/judgement-packet.js";
import type { FormationEntry } from "../../../contracts/generated/formation-window.js";
import { HostClient } from "../../host-client/src/client.js";
import { fenceFor } from "../../host-client/src/fence.js";
import { input } from "../../pi-worker/test/fixtures.js";
import { GenerativeProvider, type JudgementProvider } from "../../judgement/src/providers.js";
import { maximum } from "../../judgement/test/fixtures.js";
import { POOL_MODULES, composeApplication, runSupervisor, type Services } from "../src/index.js";

/** Composes the pool's modules for the named processes against scripted services. */
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
 * lane rules; that file is not imported from or edited). */
interface World {
  base: string; url: string; host: ReturnType<typeof spawn>; client: HostClient; admin: HostClient; pool: HostClient;
  scope: Record<string, unknown>; policy: { reference: unknown }; budget: { id: string };
  submit: (process: "investigation" | "formation", extra: Record<string, unknown>) => Promise<import("../../../contracts/generated/host-response.js").Job>;
  job: (id: string) => Promise<import("../../../contracts/generated/host-response.js").Job>;
  close: () => Promise<void>;
  otherProject: () => Promise<{ submit: World["submit"]; job: World["job"] }>;
}
async function world(): Promise<World> {
  const base = await mkdtemp(join(tmpdir(), "memory-scenario-"));
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
    action: "create_policy", label: "Scenario test", scope, effective: { kind: "unknown" },
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
    async otherProject() {
      const otherScope = { ...scope, project_id: randomUUID() };
      const otherPolicy = expectKind(await admin.request({ action: "user", request: { action: "mutate", request_id: randomUUID(), mutation: {
        action: "create_policy", label: "Other project", scope: otherScope, effective: { kind: "unknown" },
        policy: { retention_purpose: "test", allowed_uses: ["task_context"], source_rules: [], evidence_requirements: [], applicability_rules: [], budget_class: "test", scheduling_priority: 0, judgement_dispositions: [], notification_policy: "material", qualification_requirements: [] } } } }), "user").result;
      if (otherPolicy.kind !== "policy") throw new Error("policy");
      const otherBudget = { ...budget, id: randomUUID(), scope: otherScope };
      await admin.request({ action: "create_budget", budget: otherBudget });
      return {
        async submit(process: "investigation" | "formation", extra: Record<string, unknown>) {
          const task_id = randomUUID();
          const brief = { ...fixture.payload, process, task_id, scope: { ...otherScope, task_id }, policy: otherPolicy.policy.reference, purpose: `${process} test`,
            inputs: { memories: [], sources: [], artifacts: [] }, capabilities: { sources: [], queries: [], tools: [] }, known_conflicts: [], evidence_cutoff: new Date().toISOString(),
            profile: { id: randomUUID(), revision: 1, label: `${process} profile` },
            limits: { root_budget_id: otherBudget.id, max_provider_attempts: 8, max_tokens: 4_000_000, max_output_bytes: 1_000_000, max_child_depth: 0, max_child_concurrency: 0 }, ...extra };
          const command = { ...fixture, request_id: randomUUID(), tenant_id, actor_id: adminActor, scope: brief.scope, budget_id: otherBudget.id, deadline: minutes(20), payload: { brief, parent_id: null, max_attempts: 2, retain_until: minutes(1440) } };
          return expectKind(await admin.request({ action: "submit", command } as HostRequest), "job").job;
        },
        async job(id: string) { return expectKind(await admin.request({ action: "inspect_job", job_id: id }), "job").job; },
      };
    },
  };
}

/** J30 role for this scenario: only a bounded tool exchange (a tool call with its own result) holds
 * durable material; every other span (a message, a lone result) is examined and holds nothing. J01
 * takes the candidate's own evidential status as its assessment; the rest are fixed per the brief. */
function scriptedChoice(packet: JudgementPacket, key: string): string {
  const span = packet.evidence.find((e) => e.name === "span")?.content as { kind: string } | undefined;
  if (key === "J30.role") return span?.kind === "exchange" ? "knowledge" : "none";
  if (key.startsWith("J06.")) return key === "J06.contradiction" ? "no" : key === "J06.direct" ? "yes" : "no";
  if (key === "J07.assessment") return "relevant";
  if (key === "J08.assessment") return "established";
  const entry = packet.evidence.find((e) => e.name === "candidate")?.content as unknown as FormationEntry | undefined;
  const choices: Record<string, string> = {
    "J01.assessment": entry?.event.evidential_status ?? "observation",
    "J01.faithfulness": "yes",
    "J02.assessment": "supports",
    "J03.assessment": "boundary",
  };
  return choices[key];
}
/** Scripted judgement reference answering per `scriptedChoice`. A separate faux provider and model
 * from the task model's, so judgement's queued replies never clobber the task's own. */
function scriptedReference(faux: ReturnType<typeof fauxProvider>, models: ReturnType<typeof createModels>): JudgementProvider {
  const generative = new GenerativeProvider("reference", models, faux.getModel(), maximum, 2048, faux.getModel().id);
  return {
    id: generative.id, model: generative.model, release: generative.release, maximum, distributions: false,
    async evaluate(packet: JudgementPacket, signal: AbortSignal) {
      const answers = Object.fromEntries(packet.questions.flatMap((d) => d.definition.questions.map((q) => [`${d.definition.id}.${q.key}`, { type: "choice", choice: scriptedChoice(packet, `${d.definition.id}.${q.key}`) }])));
      faux.setResponses([fauxAssistantMessage(JSON.stringify({ answers }))]);
      return generative.evaluate(packet, signal);
    },
  };
}

it("a task's real tool use becomes retained knowledge through automatic formation, and reaches only a related task in its own project", async () => {
  const w = await world();
  const cwd = await mkdtemp(join(tmpdir(), "memory-scenario-cwd-"));
  try {
    const phrase = "clearance under beam B3 measured at 2.0 m";
    await writeFile(join(cwd, "site-notes.txt"), `Site notes: ${phrase}`);

    // Task A: reads the file with the real `read` tool under a node execution service, and answers.
    const taskA = await w.submit("investigation", { capabilities: { sources: [], queries: [], tools: ["read"] } });
    const faux = fauxProvider(), models = createModels();
    models.setProvider(faux.provider);
    const taskResult = {
      schema_version: "1", status: "complete", examined_scope: taskA.spec.brief.scope, inputs: taskA.spec.brief.inputs,
      findings: [], coverage: { examined: ["Read site-notes.txt for the crane route clearance"], unexamined: [] }, unresolved_work: [],
      proposed_changes: [], child_outputs: [], known_effects: [], usage: { status: "unknown", input_tokens: null, output_tokens: null, cost: null },
    };
    faux.setResponses([
      fauxAssistantMessage(fauxToolCall("read", { path: "site-notes.txt" }), { stopReason: "toolUse" }),
      fauxAssistantMessage(JSON.stringify(taskResult)),
    ]);
    // Judgement scripts its own replies on a separate faux queue; sharing the task model's queue
    // would clobber the task's own reply now that activation runs before the model does.
    const judgementFaux = fauxProvider(), judgementModels = createModels();
    judgementModels.setProvider(judgementFaux.provider);
    const reference = scriptedReference(judgementFaux, judgementModels);
    const log: string[] = [];
    const run = async (jobId: string, ...processes: ("investigation" | "formation")[]) => runSupervisor({
      host: w.pool, hostUrl: w.url, concurrency: 1, pollMs: 50, leaseSeconds: 60, sessionsDirectory: join(w.base, "sessions"),
      composition: compose({ taskModel: taskService(models, faux), judgement: { primary: reference, maximum }, execution: { kind: "node", cwd } }, ...processes),
      log: (line) => log.push(line),
      until: async () => SETTLED.includes((await w.job(jobId)).state),
    }, context);
    await run(taskA.id, "investigation");
    expect((await w.job(taskA.id)).state, log.join("\n")).toBe("completed");

    // The published experience is the durable proof the tool actually ran: a throwaway formation job
    // (separate from the one the Host schedules on its own) reads it raw and finds a tool_result span.
    const experience = expectKind(await w.client.request({ action: "experience", job_id: taskA.id }), "source").source;
    const inspectionJob = await w.submit("formation", {
      inputs: { memories: [], sources: [experience.reference], artifacts: [] },
      capabilities: { sources: [experience.reference], queries: [], tools: [] },
    });
    const claimedForInspection = await w.pool.request({ action: "claim_next", processes: ["formation"], lease_seconds: 60 });
    if (claimedForInspection.kind !== "work") throw new Error("inspection job was not claimed");
    expect(claimedForInspection.assignment.job.id).toBe(inspectionJob.id);
    const inspectionFence = fenceFor(claimedForInspection.assignment);
    const inspectionClient = new HostClient(w.url, claimedForInspection.credential.token);
    await inspectionClient.request({ action: "start", fence: inspectionFence, composition: "test" }, context.abortSignal);
    const captured = await inspectionClient.request({
      action: "formation_window", fence: inspectionFence, id: randomUUID(), source: experience.reference, operation: "inspect", limit: 32,
    }, context.abortSignal);
    if (captured.kind !== "experience_window") throw new Error(`Expected an experience window, got ${captured.kind}`);
    const toolResult = captured.window.spans.flatMap((span) => span.events).find((event) => event.kind === "tool_result");
    expect(toolResult, JSON.stringify(captured.window.spans)).toBeTruthy();
    expect(JSON.stringify(toolResult!.content)).toContain(phrase);

    // The Host schedules formation over that same experience by itself; find it through the durable
    // events rather than submitting it (nobody does).
    const scheduled = async () => {
      const page = expectKind(await w.client.request({ action: "events", after: 0, limit: 500 }), "events").page;
      for (const event of page.events.filter((e) => e.kind === "job_accepted" && e.resource_id !== taskA.id && e.resource_id !== inspectionJob.id)) {
        const job = await w.job(event.resource_id);
        if (job.spec.brief.process === "formation" && job.spec.brief.inputs.sources.some((s) => s.source_id === experience.reference.source_id)) return job;
      }
      return undefined;
    };
    const started = Date.now();
    let formation = await scheduled();
    while (!formation) {
      if (Date.now() - started > 30_000) throw new Error("the Host did not schedule formation for the settled task");
      await new Promise((d) => setTimeout(d, 500));
      formation = await scheduled();
    }
    await run(formation.id, "formation");
    const doneFormation = await w.job(formation.id);
    expect(["completed", "partial"], log.join("\n")).toContain(doneFormation.state);

    // A retained record copied the tool result's own text, keeping its observed origin and status:
    // this came from what the tool actually returned, not from the model's own assistant text.
    const records = expectKind(await w.client.request({ action: "user", request: { action: "browse", query: { include_inactive: false, limit: 50 } } }), "user").result;
    if (records.kind !== "records") throw new Error("records");
    const formed = records.records.find((r) => r.record.content.family === "knowledge" && r.record.content.statement.includes(phrase));
    expect(formed, JSON.stringify(records.records)).toBeTruthy();
    expect(formed!.record.origin).toBe("observed");
    expect(formed!.record.evidential_status).toBe("observation");

    // Task B: a fresh task in the same project with a related purpose sees the formed record's label
    // reach its model through activation at admission, given no memory IDs.
    const taskB = await w.submit("investigation", { purpose: "What clearance is available under beam B3 for the crane route?" });
    let seenByModelB = "";
    faux.setResponses([(transcript) => { seenByModelB = JSON.stringify(transcript); return fauxAssistantMessage(JSON.stringify({ ...taskResult, examined_scope: taskB.spec.brief.scope, inputs: taskB.spec.brief.inputs })); }]);
    await run(taskB.id, "investigation");
    expect((await w.job(taskB.id)).state, log.filter((l) => l.includes(taskB.id)).join("\n")).toBe("completed");
    expect(seenByModelB, log.filter((l) => l.includes(taskB.id) || l.includes("activation")).join("\n")).toContain(formed!.record.label);

    // Task C: a related-purpose task in a different project does not see it; scope, not purpose
    // similarity, bounds what activation may retrieve.
    const other = await w.otherProject();
    const taskC = await other.submit("investigation", { purpose: "What clearance is available under beam B3 for the crane route?" });
    let seenByModelC = "";
    faux.setResponses([(transcript) => { seenByModelC = JSON.stringify(transcript); return fauxAssistantMessage(JSON.stringify({ schema_version: "1", status: "complete", examined_scope: taskC.spec.brief.scope, inputs: taskC.spec.brief.inputs, findings: [], coverage: { examined: ["Scripted"], unexamined: [] }, unresolved_work: [], proposed_changes: [], child_outputs: [], known_effects: [], usage: { status: "unknown", input_tokens: null, output_tokens: null, cost: null } })); }]);
    await runSupervisor({
      host: w.pool, hostUrl: w.url, concurrency: 1, pollMs: 50, leaseSeconds: 60, sessionsDirectory: join(w.base, "sessions"),
      composition: compose({ taskModel: taskService(models, faux), judgement: { primary: reference, maximum } }, "investigation"),
      log: (line) => log.push(line),
      until: async () => SETTLED.includes((await other.job(taskC.id)).state),
    }, context);
    expect((await other.job(taskC.id)).state).toBe("completed");
    expect(seenByModelC).not.toContain(phrase);
    expect(seenByModelC).not.toContain(formed!.record.label);
  } finally {
    await w.close();
    await rm(cwd, { recursive: true, force: true });
  }
}, 180_000);
