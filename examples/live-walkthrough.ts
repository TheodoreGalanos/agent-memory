// ABOUTME: Live continuation of the user guide. Submits work for the Host's pool: a model works a
// ABOUTME: task, its findings become a source, formation judges them (Azure reference, Jev shadow).
import { existsSync, readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { randomUUID } from "node:crypto";
import type { HostRequest, Scope, SourceVersion } from "../contracts/generated/host-request.js";
import type { HostResponse, Job } from "../contracts/generated/host-response.js";
import type { WorkResult } from "../contracts/generated/work-result.js";
import { HostClient } from "../packages/pi-worker/src/host-client.js";
import { findingsToToolEvents, formatManifest } from "./live-walkthrough-support.js";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const steps = ["prepare", "work", "capture", "form", "show", "reset"] as const;
type Step = (typeof steps)[number];
const [directoryArg, step] = process.argv.slice(2);
const usage = `Usage: npm run live -- DIRECTORY ${steps.join("|")}`;
const directory = resolve(directoryArg ?? ".");
const liveDirectory = join(directory, "live");

/** The pool's models come from worker.json; this label only names them in the captured source. */
const TASK_MODEL = { provider: "azure-openai-responses", model: "gpt-5.4-mini" };
/** Formation reservations hold their maximum without cost telemetry; size the job for two providers. */
const JUDGEMENT_CALLS = 12;
const JUDGEMENT_TOKENS = 30_000;

const readJson = (path: string) => JSON.parse(readFileSync(path, "utf8"));
const saveJson = (name: string, data: unknown) =>
  writeFileSync(join(liveDirectory, `${name}.json`), JSON.stringify(data, null, 2) + "\n", { mode: 0o600 });
const say = (line = "") => console.log(line);
const minutes = (count: number) => new Date(Date.now() + count * 60_000).toISOString();

interface LiveState {
  budget?: { id: string };
  source?: { source_id: string; revision: string };
  task?: { job_id: string };
  formation?: { job_id: string };
}
const statePath = join(liveDirectory, "state.json");
const state: LiveState = {};
const saveState = () => writeFileSync(statePath, JSON.stringify(state, null, 2) + "\n", { mode: 0o600 });
let identity: { tenant_id: string; actor_id: string; scope: Scope };
let base: { policy: { reference: unknown }; memory: { reference: unknown }; corrected?: { reference: unknown } };
let memoryReference: { memory_id: string; revision: number; label: string };

/** Host errors name the failing command so a rejected step can be read without a debugger. */
function hostFor(configName: string): HostClient {
  const config = readJson(join(directory, configName)) as { endpoint: string; token: string };
  const client = new HostClient(config.endpoint, config.token);
  const inner = client.request.bind(client);
  client.request = async (command, signal) => {
    try {
      return await inner(command, signal);
    } catch (error) {
      throw new Error(`${command.action}: ${error instanceof Error ? error.message : String(error)}`);
    }
  };
  return client;
}
function expect<K extends HostResponse["kind"]>(response: HostResponse, kind: K): Extract<HostResponse, { kind: K }> {
  if (response.kind !== kind) throw new Error(`Host returned ${response.kind}; expected ${kind}`);
  return response as Extract<HostResponse, { kind: K }>;
}
const user = async (inner: Record<string, unknown>) =>
  expect(await hostFor("client.json").request({ action: "user", request: inner } as HostRequest), "user").result;

/** Briefs start from the property-location fixture so every required field is present. */
function fixtureBrief() {
  return readJson(join(root, "evals/property-location/inputs/before-correction.json")).command.payload as Record<string, unknown>;
}
async function submitJob(label: string, brief: Record<string, unknown>, scope: Scope) {
  const fixture = readJson(join(root, "evals/property-location/inputs/before-correction.json")).command;
  const command = { ...fixture, request_id: randomUUID(), tenant_id: identity.tenant_id, actor_id: identity.actor_id, scope, budget_id: state.budget!.id, deadline: minutes(120), payload: { brief, parent_id: null, max_attempts: 2, retain_until: minutes(1440) } };
  saveJson(`${label}.submit.request`, { action: "submit", command });
  return expect(await hostFor("client.json").request({ action: "submit", command }), "job").job;
}

/** Polls a job the pool is driving, printing each state change, until it settles. */
async function awaitJob(jobId: string, label: string, timeoutMinutes = 10): Promise<Job> {
  const client = hostFor("client.json");
  const deadline = Date.now() + timeoutMinutes * 60_000;
  let last = "";
  while (Date.now() < deadline) {
    const job = expect(await client.request({ action: "inspect_job", job_id: jobId }), "job").job;
    if (job.state !== last) {
      say(`${label} job ${job.id}: ${job.state}${job.attempt ? ` (attempt ${job.attempt})` : ""}`);
      last = job.state;
    }
    if (["completed", "partial", "failed", "cancelled"].includes(job.state)) return job;
    await new Promise((done) => setTimeout(done, 2000));
  }
  throw new Error(`${label} job ${jobId} did not settle within ${timeoutMinutes} minutes; is the Host running with a worker pool (npm run memory -- enable-pool)?`);
}

/** The Host's ledger is the spend record; model-reported usage inside a result is not. */
async function reportBudget() {
  if (!state.budget) return;
  const usage = expect(await hostFor("client.json").request({ action: "budget_usage", budget_id: state.budget.id }), "budget_usage").usage;
  const c = usage.committed, u = usage.unresolved;
  say(`Budget ledger: committed ${c.provider_calls} call(s), ${c.tokens} tokens, ${c.cost_microunits} cost microunits; unresolved reservations ${u.provider_calls} call(s), ${u.tokens} tokens.`);
}

async function prepare() {
  if (!existsSync(join(directory, "worker.json")))
    say("Warning: no worker.json; run `npm run memory -- enable-pool DIRECTORY` and restart the Host, or nothing will claim the job.");
  if (!state.budget) {
    const budget = {
      id: randomUUID(), scope: identity.scope,
      limit: { tokens: 2_000_000, provider_calls: 30, output_bytes: 2_000_000, cost_microunits: 2_000_000, sandbox_cpu_ms: 0, sandbox_time_ms: 0 },
      final_result_reserve: { tokens: 1000, provider_calls: 0, output_bytes: 8192, cost_microunits: 0, sandbox_cpu_ms: 0, sandbox_time_ms: 0 },
      deadline: minutes(180), max_child_depth: 0, max_child_concurrency: 0, pricing_revision: "live-walkthrough/list-price-proxy",
    };
    state.budget = expect(await hostFor("admin.json").request({ action: "create_budget", budget }), "budget").budget;
    saveState();
  }
  say(`Budget ${state.budget.id}: at most 30 provider calls and 2.000000 cost units; reservations settle to observed usage.`);
  state.source ??= { source_id: randomUUID(), revision: "live-1" };
  if (!state.task) {
    const scope = { ...identity.scope, task_id: randomUUID() };
    const brief = {
      ...fixtureBrief(), task_id: scope.task_id, scope, process: "investigation", policy: base.policy.reference,
      profile: { id: randomUUID(), revision: 1, label: "live walkthrough task" },
      purpose: `Report the fire-resistance rating currently recorded for wall W-101, the evidence it rests on, and what remains unverified. Use only the supplied workspace memories.`,
      inputs: { memories: [memoryReference], sources: [], artifacts: [] },
      capabilities: { sources: [], queries: [], tools: [] },
      known_conflicts: [], evidence_cutoff: new Date().toISOString(),
      limits: { root_budget_id: state.budget.id, max_provider_attempts: 2, max_tokens: 1_000_000, max_output_bytes: 65_536, max_child_depth: 0, max_child_concurrency: 0 },
    };
    const job = await submitJob("task", brief, scope);
    state.task = { job_id: job.id };
  }
  saveState();
  say(`Task job ${state.task.job_id} queued (investigation; input memory "${memoryReference.label}" revision ${memoryReference.revision}). The pool claims it within a second.`);
  say("Next: npm run live -- DIRECTORY work");
}

async function work() {
  if (!state.task) throw new Error("Run prepare first");
  const job = await awaitJob(state.task.job_id, "Task");
  const inspected = await user({ action: "inspect_task", job_id: job.id, after_manifest: null });
  const manifests = inspected.kind === "task" ? inspected.manifests : [];
  say(`Recorded context (${manifests.length} render manifest(s)):`);
  for (const manifest of manifests) say(formatManifest(manifest));
  saveJson("work", { job, manifests });
  if (!job.result) { say(`Job ${job.state} without a result; see the Host terminal for the pool's log.`); process.exitCode = 1; return; }
  const result = job.result as WorkResult;
  say();
  say(`Result status ${result.status}; ${result.findings.length} finding(s)`);
  for (const [index, finding] of result.findings.entries())
    say(`  ${index + 1}. [${finding.origin}/${finding.evidential_status}] ${finding.statement}\n     supported by ${finding.supporting_memories.map((m) => `${m.label} r${m.revision}`).join(", ") || "no memory references"}`);
  say(`  examined:   ${result.coverage.examined.join("; ") || "-"}`);
  say(`  unexamined: ${result.coverage.unexamined.join("; ") || "-"}`);
  say(`  unresolved: ${result.unresolved_work.join("; ") || "-"}`);
  await reportBudget();
  say();
  say(`Inspect the recorded context: npm run memory -- task --config ${join(directory, "client.json")} --id ${job.id}`);
}

async function capture() {
  if (!state.task || !state.source) throw new Error("Run prepare and work first");
  const client = hostFor("client.json");
  const job = expect(await client.request({ action: "inspect_job", job_id: state.task.job_id }), "job").job;
  if (!job.result) throw new Error("The task job has no published result yet; run work first");
  const result = job.result as WorkResult;
  if (result.findings.length === 0) {
    say("The model published no validated findings, so there is nothing to capture. Formation has no candidates from this run.");
    process.exitCode = 1;
    return;
  }
  const events = findingsToToolEvents(job, result, TASK_MODEL);
  const source = {
    reference: state.source, label: `W-101 task findings: job ${job.id}`, scope: identity.scope, kind: "tool_events" as const,
    owner: "examples/live-walkthrough.ts connector", acquired_at: new Date().toISOString(),
    acquisition_method: `Published WorkResult of job ${job.id}, converted finding by finding`, precedence: null, snapshot_artifact: null,
  };
  const command: HostRequest = { action: "ingest_source", source, content: { schema_version: "memory-tool-events/1", events } };
  const publishedPath = join(liveDirectory, "capture.response.json");
  let published: SourceVersion;
  if (existsSync(publishedPath)) {
    published = readJson(publishedPath);
    say("Source already published by an earlier capture; reusing its revision");
  } else {
    saveJson("capture.request", command);
    published = expect(await client.request(command), "source").source;
    saveJson("capture.response", published);
  }
  say(`Published source ${published.reference.source_id} revision ${published.reference.revision}; snapshot artifact ${published.snapshot_artifact}`);
  for (const event of events)
    say(`  ${event.event_id}: [${event.origin}/${event.evidential_status}] ${(event.content as { content: { statement: string } }).content.statement}`);
  say("Each event keeps the finding's own origin and evidential status; the connector adds no verification.");
  if (!state.formation) {
    const scope = { ...identity.scope, task_id: randomUUID() };
    const brief = {
      ...fixtureBrief(), task_id: scope.task_id, scope, process: "formation", policy: base.policy.reference,
      profile: { id: randomUUID(), revision: 1, label: "live walkthrough formation" },
      purpose: `Decide which findings from job ${job.id} become candidate memories.`,
      inputs: { memories: [], sources: [state.source], artifacts: [] },
      capabilities: { sources: [state.source], queries: [], tools: [] },
      // Assessments must be fresher than the cutoff; the source events are already recorded.
      known_conflicts: [], evidence_cutoff: new Date().toISOString(),
      limits: { root_budget_id: state.budget!.id, max_provider_attempts: JUDGEMENT_CALLS, max_tokens: JUDGEMENT_CALLS * JUDGEMENT_TOKENS, max_output_bytes: 200_000, max_child_depth: 0, max_child_concurrency: 0 },
    };
    const formation = await submitJob("formation", brief, scope);
    state.formation = { job_id: formation.id };
    saveState();
  }
  say(`Formation job ${state.formation.job_id} queued; it reads source ${state.source.source_id} revision ${state.source.revision}. Watch the Host terminal for the judgement trace.`);
  say("Next: npm run live -- DIRECTORY form");
}

async function form() {
  if (!state.formation) throw new Error("Run prepare, work and capture first");
  const job = await awaitJob(state.formation.job_id, "Formation");
  saveJson("form", { job });
  if (job.result) {
    const result = job.result as WorkResult;
    say(`Result status ${result.status}; unresolved: ${result.unresolved_work.join("; ") || "-"}`);
  }
  const records = await user({ action: "browse", query: { include_inactive: false, limit: 50 } });
  if (records.kind === "records") {
    const derived = records.records.filter((r) => r.record.label.startsWith(`${state.task?.job_id}:`));
    say(`Retained ${derived.length} candidate record(s) from the task's findings:`);
    for (const item of derived)
      say(`  + ${item.record.label} r${item.reference.revision} [${item.record.content.family}, ${item.record.evidential_status}, ${item.record.qualification.status}]`);
  }
  say("The per-question Jev and Azure answers are in the Host terminal (pool trace). Jev ran in shadow mode and did not decide retention.");
  await reportBudget();
}

async function show() {
  const records = await user({ action: "browse", query: { include_inactive: false, limit: 50 } });
  if (records.kind !== "records") throw new Error("records");
  say(`Current records in scope (${records.records.length}):`);
  for (const item of records.records)
    say(`  - ${item.record.label} r${item.reference.revision} [${item.record.origin}/${item.record.evidential_status}, ${item.record.qualification.status}] created by ${item.created_by}${item.record.source_locators.length ? `; ${item.record.source_locators.length} source locator(s)` : ""}`);
  if (state.task) {
    const task = await user({ action: "inspect_task", job_id: state.task.job_id, after_manifest: null });
    if (task.kind === "task") say(`Task ${state.task.job_id}: ${task.manifests.length} recorded render manifest(s); ${task.coverage.unexamined.join("; ") || "no unexamined notes"}`);
  }
  const changes = await user({ action: "changes", after: 0, limit: 100 });
  if (changes.kind === "changes") {
    const kinds = new Map<string, number>();
    for (const event of changes.page.events) kinds.set(event.kind, (kinds.get(event.kind) ?? 0) + 1);
    say(`Change events: ${[...kinds].map(([kind, count]) => `${kind} ×${count}`).join(", ") || "none"}`);
  }
  saveJson("show", { records, changes });
  say();
  say(`Full detail: npm run memory -- browse|changes|history|task --config ${join(directory, "client.json")} ...`);
}

/** Cancels the live jobs and forgets them; the budget and published sources remain. */
async function reset() {
  const client = hostFor("client.json");
  for (const live of [state.task, state.formation]) {
    if (!live) continue;
    const job = expect(await client.request({ action: "inspect_job", job_id: live.job_id }), "job").job;
    if (["queued", "leased", "running", "waiting"].includes(job.state)) {
      await client.request({ action: "cancel", job_id: job.id });
      say(`Cancelled job ${job.id} (${job.state})`);
    } else say(`Job ${job.id} is ${job.state}; left as recorded history`);
  }
  delete state.task;
  delete state.formation;
  state.source = { source_id: randomUUID(), revision: "live-1" };
  saveState();
  say("Live state cleared. Run prepare again.");
}

const run: Record<Step, () => Promise<void>> = { prepare, work, capture, form, show, reset };
async function main() {
  if (!directoryArg || !steps.includes(step as Step)) throw new Error(usage);
  const basePath = join(directory, "walkthrough.json");
  if (!existsSync(basePath))
    throw new Error(`Run the provider-free walkthrough first (seed, correct): ${basePath} is missing`);
  base = readJson(basePath);
  identity = (base as unknown as { identity: typeof identity }).identity;
  memoryReference = ((base.corrected ?? base.memory).reference) as typeof memoryReference;
  mkdirSync(liveDirectory, { recursive: true, mode: 0o700 });
  if (existsSync(statePath)) Object.assign(state, readJson(statePath));
  await run[step as Step]();
}
main().catch((error: unknown) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
});
