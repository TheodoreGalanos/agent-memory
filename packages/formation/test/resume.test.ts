// ABOUTME: Tests that formation over a long experience source resumes across an interrupted attempt
// ABOUTME: without re-judging already-decided spans, and that a differently-purposed pass restarts.
import { spawn } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { once } from "node:events";
import { createServer } from "node:net";
import { randomBytes, randomUUID } from "node:crypto";
import { BACKGROUND_CONTEXT, withCancel } from "@earendil-works/pi-agent-core";
import { createModels, fauxProvider, fauxAssistantMessage } from "@earendil-works/pi-ai";
import { expect, it } from "vitest";
import type { HostRequest, Scope } from "../../../contracts/generated/host-request.js";
import type { HostResponse } from "../../../contracts/generated/host-response.js";
import type { JudgementPacket } from "../../../contracts/generated/judgement-packet.js";
import { HostClient } from "../../host-client/src/client.js";
import { input } from "../../pi-worker/test/fixtures.js";
import { GenerativeProvider, type JudgementProvider } from "../../judgement/src/providers.js";
import { maximum } from "../../judgement/test/fixtures.js";
import { POOL_MODULES, composeApplication, runSupervisor, type Services } from "../../supervisor/src/index.js";

/** Composes the pool's formation module against scripted services, mirroring supervisor.test.ts's
 * own helper (kept local per the lane rule against editing that file). */
function compose(services: Services) {
  return composeApplication(services, POOL_MODULES.filter((m) => m.declaration.operations.includes("formation")));
}

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
  base: string; url: string; host: ReturnType<typeof spawn>; client: HostClient; pool: HostClient;
  scope: Scope; policy: { reference: unknown }; budget: { id: string };
  submit: (extra: Record<string, unknown>) => Promise<import("../../../contracts/generated/host-response.js").Job>;
  job: (id: string) => Promise<import("../../../contracts/generated/host-response.js").Job>;
  records: () => Promise<number>;
  close: () => Promise<void>;
}
/** Copied from packages/supervisor/test/supervisor.test.ts's own `world()` helper per the lane rule
 * against importing from or editing that file: a pool credential against a real, freshly spawned Host. */
async function world(): Promise<World> {
  const base = await mkdtemp(join(tmpdir(), "memory-formation-resume-"));
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
    action: "create_policy", label: "Resume test", scope, effective: { kind: "unknown" },
    policy: { retention_purpose: "test", allowed_uses: ["task_context"], source_rules: [], evidence_requirements: [], applicability_rules: [], budget_class: "test", scheduling_priority: 0, judgement_dispositions: [], notification_policy: "material", qualification_requirements: [] } } } }), "user").result;
  if (created.kind !== "policy") throw new Error("policy");
  const budget = { id: randomUUID(), scope, limit: { tokens: 20_000_000, provider_calls: 5000, output_bytes: 20_000_000, cost_microunits: 20_000_000, sandbox_cpu_ms: 0, sandbox_time_ms: 0 }, final_result_reserve: { tokens: 100, provider_calls: 0, output_bytes: 1000, cost_microunits: 0, sandbox_cpu_ms: 0, sandbox_time_ms: 0 }, deadline: minutes(30), max_child_depth: 0, max_child_concurrency: 0, pricing_revision: "test" };
  await admin.request({ action: "create_budget", budget });
  return {
    base, url, host, client, pool: new HostClient(url, tokens.pool), scope, policy: { reference: created.policy.reference }, budget,
    async submit(extra) {
      const task_id = randomUUID();
      const brief = { ...fixture.payload, process: "formation", task_id, scope: { ...scope, task_id }, policy: created.policy.reference, purpose: "resume test",
        inputs: { memories: [], sources: [], artifacts: [] }, capabilities: { sources: [], queries: [], tools: [] }, known_conflicts: [], evidence_cutoff: new Date().toISOString(),
        profile: { id: randomUUID(), revision: 1, label: "formation profile" },
        limits: { root_budget_id: budget.id, max_provider_attempts: 1000, max_tokens: 16_000_000, max_output_bytes: 4_000_000, max_child_depth: 0, max_child_concurrency: 0 }, ...extra };
      const command = { ...fixture, request_id: randomUUID(), tenant_id, actor_id, scope: brief.scope, budget_id: budget.id, deadline: minutes(20), payload: { brief, parent_id: null, max_attempts: 2, retain_until: minutes(1440) } };
      return expectKind(await client.request({ action: "submit", command } as HostRequest), "job").job;
    },
    async job(id) { return expectKind(await client.request({ action: "inspect_job", job_id: id }), "job").job; },
    async records() {
      const browse = expectKind(await client.request({ action: "user", request: { action: "browse", query: { include_inactive: false, limit: 200 } } }), "user").result;
      if (browse.kind !== "records") throw new Error("records");
      return browse.records.length;
    },
    async close() { host.kill("SIGINT"); await once(host, "exit"); await rm(base, { recursive: true, force: true }); },
  };
}

/** ~40 spans of alternating tool_call/tool_result events, each result carrying several KB of
 * distinct text so a handful of spans already exceed the Host's one-window byte bound. */
function experienceEvents(count: number, textBytes: number): unknown[] {
  const base = Date.now() - 60 * 60 * 1000;
  const events: unknown[] = [];
  for (let i = 0; i < count; i++) {
    const at = new Date(base + i * 1000).toISOString();
    events.push({ event_id: `${i}-call`, observed_at: at, kind: "tool_call", origin: "agent_generated", evidential_status: "inference", content: { tool: "read", call_id: `c${i}`, arguments: { path: `notes-${i}.txt` } } });
    events.push({
      event_id: `${i}-result`, observed_at: at, kind: "tool_result", origin: "observed", evidential_status: "observation",
      content: { tool: "read", call_id: `c${i}`, is_error: false, truncated: false, text: `Finding ${i}: clearance under beam B${i} measured at ${2 + (i % 5) / 10} m. ${"detail ".repeat(Math.ceil(textBytes / 7))}`.slice(0, textBytes) },
    });
  }
  return events;
}
async function ingestExperience(client: HostClient, scope: Scope, count: number, textBytes: number) {
  const source = { source_id: randomUUID(), revision: "1" };
  await client.request({
    action: "ingest_source",
    source: { reference: source, label: "Experience", scope, kind: "tool_events", owner: "test", acquired_at: new Date().toISOString(), acquisition_method: "test", precedence: null, snapshot_artifact: null },
    content: { schema_version: "memory-tool-events/1", events: experienceEvents(count, textBytes) },
  });
  return source;
}

/** J30 judges every exchange span as holding knowledge; a retained candidate's own evidential status
 * answers J01, and J02/J03 are unconditionally supported/boundary, matching supervisor.test.ts's pattern. */
function scriptedChoice(packet: JudgementPacket, key: string): string {
  if (key === "J30.role") {
    const span = packet.evidence.find((e) => e.name === "span")?.content as { kind: string } | undefined;
    return span?.kind === "exchange" ? "knowledge" : "none";
  }
  const entry = packet.evidence.find((e) => e.name === "candidate")?.content as { event: { evidential_status: string } } | undefined;
  const choices: Record<string, string> = { "J01.assessment": entry?.event.evidential_status ?? "observation", "J01.faithfulness": "yes", "J02.assessment": "supports", "J03.assessment": "boundary" };
  return choices[key];
}
/** A judgement provider scripted from each packet's own evidence; `onCall` may record calls or
 * inject a fault before the reply is produced. */
function scriptedProvider(faux: ReturnType<typeof fauxProvider>, models: ReturnType<typeof createModels>, onCall: (packet: JudgementPacket, signal: AbortSignal) => void | Promise<void>): JudgementProvider {
  const generative = new GenerativeProvider("primary", models, faux.getModel(), maximum, 4096, faux.getModel().id);
  return {
    id: generative.id, model: generative.model, release: generative.release, maximum, distributions: false,
    async evaluate(packet: JudgementPacket, signal: AbortSignal) {
      await onCall(packet, signal);
      const answers = Object.fromEntries(packet.questions.flatMap((d) => d.definition.questions.map((q) => [`${d.definition.id}.${q.key}`, { type: "choice", choice: scriptedChoice(packet, `${d.definition.id}.${q.key}`) }])));
      faux.setResponses([fauxAssistantMessage(JSON.stringify({ answers }))]);
      return generative.evaluate(packet, signal);
    },
  };
}
function isRolePacket(packet: JudgementPacket): boolean {
  return packet.questions.length === 1 && packet.questions[0].definition.id === "J30";
}
/** The absolute, source-wide position a role packet's span occupies: stable across whichever
 * window happened to capture it, unlike the span's own window-local `index`. */
function rolePosition(packet: JudgementPacket): number {
  const span = packet.evidence.find((e) => e.name === "span")?.content as { start: number } | undefined;
  if (span === undefined) throw new Error("Role packet carries no span evidence");
  return span.start;
}

it("interrupted multi-window capture resumes from the Host's cursor; a new purpose re-reads from the start", async () => {
  const w = await world();
  try {
    const SPANS = 40, TEXT_BYTES = 250;
    const source = await ingestExperience(w.client, w.scope, SPANS, TEXT_BYTES);
    const job = await w.submit({ purpose: "resume test", inputs: { memories: [], sources: [source], artifacts: [] }, capabilities: { sources: [source], queries: [], tools: [] } });

    const roleCalls: { windowId: string; position: number }[] = [];
    // Attempt 1: the primary answers correctly for the first two role decisions of the *second*
    // distinct window it sees, then fails - after a whole first window already committed cleanly,
    // this interrupts the job mid-window rather than at a window boundary.
    const { context: attempt1Context, cancel: interrupt } = withCancel(BACKGROUND_CONTEXT);
    const seenWindows: string[] = [];
    const countInWindow = new Map<string, number>();
    let faulted = false;
    const faux1 = fauxProvider(), models1 = createModels();
    models1.setProvider(faux1.provider);
    const primary1 = scriptedProvider(faux1, models1, (packet) => {
      if (!isRolePacket(packet)) return;
      const windowId = packet.evidence.find((e) => e.name === "span")!.artifact_id;
      if (!seenWindows.includes(windowId)) seenWindows.push(windowId);
      if (seenWindows.length < 2) {
        roleCalls.push({ windowId, position: rolePosition(packet) });
        return;
      }
      if (windowId !== seenWindows[1]) return; // a third window must never start before the fault
      const n = (countInWindow.get(windowId) ?? 0) + 1;
      countInWindow.set(windowId, n);
      if (n > 2 && !faulted) {
        faulted = true;
        interrupt();
        throw new Error("scripted judgement outage");
      }
      roleCalls.push({ windowId, position: rolePosition(packet) });
    });
    const log1: string[] = [];
    await runSupervisor({
      host: w.pool, hostUrl: w.url, concurrency: 1, pollMs: 50, leaseSeconds: 60, sessionsDirectory: join(w.base, "sessions"),
      composition: compose({ judgement: { primary: primary1, maximum } }), log: (line) => log1.push(line),
      until: async () => faulted || SETTLED.includes((await w.job(job.id)).state),
    }, attempt1Context);
    expect(faulted, log1.join("\n")).toBe(true);
    expect(seenWindows.length).toBeGreaterThanOrEqual(2);
    if (!attempt1Context.abortSignal?.aborted) interrupt();

    // The interrupted attempt never reached "complete": no records exist beyond whatever window(s)
    // had already committed before the fault, and the job's own attempt did not finish cleanly.
    const afterAttempt1 = await w.job(job.id);
    expect(afterAttempt1.state, log1.join("\n")).not.toBe("completed");
    expect(afterAttempt1.attempt).toBe(1);
    const recordsAfterAttempt1 = await w.records();
    expect(recordsAfterAttempt1).toBeGreaterThan(0);
    expect(recordsAfterAttempt1).toBeLessThan(SPANS);

    // Recovery: the Host's own timer notices the shortened lease and requeues the job for a second attempt.
    const started = Date.now();
    for (;;) {
      const state = (await w.job(job.id)).state;
      if (state === "queued") break;
      if (Date.now() - started > 30_000) throw new Error(`recovery did not requeue the interrupted job (state=${state})`);
      await new Promise((d) => setTimeout(d, 300));
    }

    // Attempt 2: a healthy primary. Formation resumes; the job settles with nothing remaining.
    const faux2 = fauxProvider(), models2 = createModels();
    models2.setProvider(faux2.provider);
    const primary2 = scriptedProvider(faux2, models2, (packet) => {
      if (!isRolePacket(packet)) return;
      roleCalls.push({ windowId: packet.evidence.find((e) => e.name === "span")!.artifact_id, position: rolePosition(packet) });
    });
    const log2: string[] = [];
    await runSupervisor({
      host: w.pool, hostUrl: w.url, concurrency: 1, pollMs: 50, leaseSeconds: 60, sessionsDirectory: join(w.base, "sessions"),
      composition: compose({ judgement: { primary: primary2, maximum } }), log: (line) => log2.push(line),
      until: async () => SETTLED.includes((await w.job(job.id)).state),
    }, BACKGROUND_CONTEXT);
    const done = await w.job(job.id);
    // A multi-window job legitimately reports "partial": each non-final window's own commit marks
    // its own unexamined tail, even once a later window covers it - completed or partial is fine
    // as long as nothing remains (checked below, via every span being accounted for) and the job
    // itself settled rather than staying queued or leased.
    expect(["completed", "partial"], log2.join("\n")).toContain(done.state);
    expect(done.attempt).toBe(2);

    // No span was judged twice: every source position appears exactly once across both attempts.
    const byPosition = new Map<number, number>();
    for (const call of roleCalls) byPosition.set(call.position, (byPosition.get(call.position) ?? 0) + 1);
    expect(byPosition.size).toBe(SPANS);
    expect([...byPosition.values()].every((n) => n === 1), JSON.stringify([...byPosition.entries()].filter(([, n]) => n !== 1))).toBe(true);

    // Every span was eventually accounted for (retained, since all 40 are exchanges) and nothing
    // was retained twice.
    const totalRecords = await w.records();
    expect(totalRecords).toBe(SPANS);

    // A second job over the same source but a different purpose reinterprets it: the cursor for
    // that purpose starts at zero, so every span is judged again from the start.
    const reinterpretCalls: number[] = [];
    const faux3 = fauxProvider(), models3 = createModels();
    models3.setProvider(faux3.provider);
    const primary3 = scriptedProvider(faux3, models3, (packet) => {
      if (!isRolePacket(packet)) return;
      reinterpretCalls.push(rolePosition(packet));
    });
    const reinterpret = await w.submit({ purpose: "a different reinterpretation of the same experience", inputs: { memories: [], sources: [source], artifacts: [] }, capabilities: { sources: [source], queries: [], tools: [] } });
    const log3: string[] = [];
    await runSupervisor({
      host: w.pool, hostUrl: w.url, concurrency: 1, pollMs: 50, leaseSeconds: 60, sessionsDirectory: join(w.base, "sessions"),
      composition: compose({ judgement: { primary: primary3, maximum } }), log: (line) => log3.push(line),
      until: async () => SETTLED.includes((await w.job(reinterpret.id)).state),
    }, BACKGROUND_CONTEXT);
    const doneReinterpret = await w.job(reinterpret.id);
    expect(["completed", "partial"], log3.join("\n")).toContain(doneReinterpret.state);
    // Every span was re-read from position zero: the new purpose's own cursor had nothing recorded
    // yet, so its candidates are freshly retained rather than found duplicate against the first
    // purpose's records (duplicate_events is a per-window field, not surfaced on the job's own
    // aggregate result; a distinct purpose is a distinct operation cursor, confirmed below by the
    // record count growing rather than staying at the first purpose's total).
    expect(new Set(reinterpretCalls).size).toBe(SPANS);
    expect(await w.records()).toBe(SPANS * 2);
  } finally {
    await w.close();
  }
}, 180_000);
