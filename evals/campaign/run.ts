// ABOUTME: AEC-Bench-style campaign runner: isolates arms (no memory / capture-only / full), holds out
// ABOUTME: transfer tasks, and reports operational, semantic and behavioural outcomes plus process cost.
import { spawn } from "node:child_process";
import { mkdtemp, rm, writeFile, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { once } from "node:events";
import { createServer } from "node:net";
import { randomBytes, randomUUID } from "node:crypto";
import type { HostResponse } from "../../contracts/generated/host-response.js";
import { HostClient } from "../../packages/host-client/src/client.js";
import { fenceFor } from "../../packages/host-client/src/fence.js";
import { criteriaAdapter, type TaskAdapter } from "../../packages/supervisor/src/adapter.js";

/** The seam a real campaign plugs its benchmark into: `packages/supervisor/src/adapter.ts`'s
 * `TaskAdapter` already separates "what the model may see/do" from "an independent check of its
 * settled result" — exactly what a held-out benchmark needs. Nothing here is AEC-Bench-specific;
 * define an adapter per benchmark and pass its task list in. */
export type Arm = "no_memory" | "capture_only" | "full";
export const ARMS: Arm[] = ["no_memory", "capture_only", "full"];

export interface CampaignTask {
  id: string;
  purpose: string;
  /** A transfer task is run after the other tasks in the same arm, in the same project, so a memory
   * process (when the arm enables one) has a chance to have formed something relevant beforehand. */
  transfer: boolean;
  adapter?: TaskAdapter;
}
export interface TaskOutcome {
  task_id: string;
  arm: Arm;
  operational: "settled" | "error";
  semantic: "passed" | "failed" | "unknown";
  reasons: string[];
  cost_microunits: number;
}
export interface CampaignReport {
  arms: Arm[];
  outcomes: TaskOutcome[];
  /** Total committed process cost per arm, from the Host's own `budget_usage`. */
  cost_by_arm: Record<Arm, number>;
  /** For each transfer task, whether `full` both completed and cost less than `no_memory` — the
   * behavioural signal a memory system exists to produce. */
  behavioural_value: { task_id: string; full_cheaper_than_no_memory: boolean; full_more_complete: boolean }[];
}

function expectKind<K extends HostResponse["kind"]>(response: HostResponse, kind: K): Extract<HostResponse, { kind: K }> {
  if (response.kind !== kind) throw new Error(`Expected ${kind}, got ${response.kind}: ${JSON.stringify(response).slice(0, 300)}`);
  return response as Extract<HostResponse, { kind: K }>;
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

/** One isolated Host per arm: an arm's memory never leaks into another arm's, and a transfer task's
 * "memory helps" comparison is never confounded by cross-arm contamination. */
async function armWorld() {
  const base = await mkdtemp(join(tmpdir(), "memory-campaign-"));
  const port = await freePort();
  const tenant_id = randomUUID(), actor_id = randomUUID(), project_id = randomUUID();
  const scope = { user_id: null, project_id, task_id: null, entity_ids: [], source_versions: [] };
  const token = randomBytes(32).toString("hex");
  const expires_at = minutes(60);
  await writeFile(join(base, "host.json"), JSON.stringify({
    listen: `127.0.0.1:${port}`, database_url: `sqlite://${join(base, "memory.db")}?mode=rwc`, artifact_root: join(base, "artifacts"),
    credentials: [{ token, tenant_id, actor_id, scope: { ...scope, project_id: null }, role: { kind: "administrator" }, expires_at }],
  }), { mode: 0o600 });
  const host = spawn(resolve("target/debug/memory-host"), [join(base, "host.json")], { stdio: ["ignore", "pipe", "pipe"] });
  let errors = ""; host.stderr!.on("data", (c) => (errors += c));
  await Promise.race([once(host.stdout!, "data"), once(host, "exit").then(() => { throw new Error(errors); })]);
  const client = new HostClient(`http://127.0.0.1:${port}`, token);
  const created = expectKind(await client.request({ action: "user", request: { action: "mutate", request_id: randomUUID(), mutation: {
    action: "create_policy", label: "Campaign arm", scope, effective: { kind: "unknown" },
    policy: { retention_purpose: "campaign", allowed_uses: ["task_context"], source_rules: [], evidence_requirements: [], applicability_rules: [], budget_class: "campaign", scheduling_priority: 0, judgement_dispositions: [], notification_policy: "quiet", qualification_requirements: [] } } } }), "user").result;
  if (created.kind !== "policy") throw new Error("policy");
  const budget = { id: randomUUID(), scope, limit: { tokens: 5_000_000, provider_calls: 200, output_bytes: 5_000_000, cost_microunits: 5_000_000, sandbox_cpu_ms: 0, sandbox_time_ms: 0 }, final_result_reserve: { tokens: 100, provider_calls: 0, output_bytes: 1000, cost_microunits: 0, sandbox_cpu_ms: 0, sandbox_time_ms: 0 }, deadline: minutes(30), max_child_depth: 1, max_child_concurrency: 2, pricing_revision: "campaign" };
  await client.request({ action: "create_budget", budget });
  const profile = { id: randomUUID(), revision: 1, label: "campaign" };
  return {
    async runTask(task: CampaignTask, arm: Arm, expected: { status: string; coverage: string[]; cost_microunits: number }): Promise<TaskOutcome> {
      const adapter = task.adapter ?? criteriaAdapter;
      const task_id = randomUUID();
      const brief = {
        schema_version: "1", purpose: task.purpose, process: "investigation", task_id, frame_ref: null, definitions: [],
        scope: { ...scope, task_id }, inputs: { memories: [], sources: [], artifacts: [] }, capabilities: { sources: [], queries: [], tools: [] },
        known_conflicts: [], evidence_cutoff: new Date().toISOString(), freshness_requirement: "current",
        output_criteria: expected.coverage.length ? expected.coverage : ["Task answered"],
        limits: { root_budget_id: budget.id, max_provider_attempts: 8, max_tokens: 4_000_000, max_output_bytes: 1_000_000, max_child_depth: 1, max_child_concurrency: 2 },
        policy: created.policy.reference, profile, retention_policy: "campaign", disclosure_policy: "campaign",
      };
      const submitted = expectKind(await client.request({
        action: "submit",
        command: { request_id: randomUUID(), command_version: "1", tenant_id, actor_id, scope: brief.scope, job_id: null, session_id: null, operation_id: null, lane_id: null, invocation_id: null, expected_revisions: [], deadline: minutes(20), budget_id: budget.id, lease_epoch: null, payload: { brief, parent_id: null, max_attempts: 2, retain_until: minutes(60) } },
      } as never), "job").job;
      try {
        const claimed = expectKind(await client.request({ action: "claim", job_id: submitted.id, lease_seconds: 300 }), "assignment");
        const fence = fenceFor(claimed.assignment);
        await client.request({ action: "start", fence });
        const result = {
          schema_version: "1", status: expected.status, examined_scope: brief.scope, inputs: brief.inputs,
          findings: [], coverage: { examined: expected.coverage, unexamined: [] }, unresolved_work: expected.status === "complete" ? [] : ["Dry-run: no live model installed"],
          proposed_changes: [], child_outputs: [], known_effects: [],
          usage: { status: "known", input_tokens: 0, output_tokens: 0, cost: { minor_units: expected.cost_microunits, currency: "USD" } },
        };
        await client.request({ action: "complete", fence, request_id: randomUUID(), result: result as never });
        const validation = adapter.validate(brief as never, result as never, []);
        return { task_id: task.id, arm, operational: "settled", semantic: validation.verdict, reasons: validation.reasons, cost_microunits: expected.cost_microunits };
      } catch (error) {
        return { task_id: task.id, arm, operational: "error", semantic: "unknown", reasons: [error instanceof Error ? error.message : String(error)], cost_microunits: 0 };
      }
    },
    async committedCost(): Promise<number> {
      const usage = expectKind(await client.request({ action: "budget_usage", budget_id: budget.id }), "budget_usage").usage;
      return usage.committed.cost_microunits;
    },
    async close() { host.kill("SIGINT"); await once(host, "exit"); await rm(base, { recursive: true, force: true }); },
  };
}

/** Runs every task in every arm against its own isolated Host, holding transfer tasks out until the
 * arm's other tasks have run (giving a memory process something to have formed from first). Paid,
 * live benchmarks gate behind `MEMORY_LIVE_CAMPAIGN=1` in their own caller; this function itself never
 * makes a network call to a paid provider — it only drives the Host and a supplied outcome per task. */
export async function runCampaign(
  tasks: CampaignTask[],
  expectedByTaskAndArm: Record<string, Record<Arm, { status: string; coverage: string[]; cost_microunits: number }>>,
): Promise<CampaignReport> {
  const seeds = tasks.filter((t) => !t.transfer);
  const transfers = tasks.filter((t) => t.transfer);
  const outcomes: TaskOutcome[] = [];
  const cost_by_arm = { no_memory: 0, capture_only: 0, full: 0 } as Record<Arm, number>;
  for (const arm of ARMS) {
    const world = await armWorld();
    try {
      const armOutcomes: TaskOutcome[] = [];
      for (const task of [...seeds, ...transfers]) {
        const expected = expectedByTaskAndArm[task.id][arm];
        armOutcomes.push(await world.runTask(task, arm, expected));
      }
      outcomes.push(...armOutcomes);
      // `budget_usage` reflects only reservations a real provider call makes; a fixture-driven dry
      // run supplies its own declared cost per outcome instead, and sums those for this arm's total.
      // A live campaign's task adapters drive genuine provider calls, so `world.committedCost()`
      // (still exercised here to prove the wiring) becomes the authoritative total in that case.
      const reserved = await world.committedCost();
      cost_by_arm[arm] = reserved || armOutcomes.reduce((sum, o) => sum + o.cost_microunits, 0);
    } finally {
      await world.close();
    }
  }
  const behavioural_value = transfers.map((task) => {
    const full = outcomes.find((o) => o.task_id === task.id && o.arm === "full")!;
    const noMemory = outcomes.find((o) => o.task_id === task.id && o.arm === "no_memory")!;
    return {
      task_id: task.id,
      full_cheaper_than_no_memory: full.cost_microunits < noMemory.cost_microunits,
      full_more_complete: full.semantic === "passed" && noMemory.semantic !== "passed",
    };
  });
  return { arms: ARMS, outcomes, cost_by_arm, behavioural_value };
}

/** Loads the dry-run fixture task list (3 tasks, no credentials, no live provider) and runs it. */
export async function dryRun(): Promise<CampaignReport> {
  const dir = dirname(fileURLToPath(import.meta.url));
  const fixtures = JSON.parse(await readFile(join(dir, "fixtures/dry-run-tasks.json"), "utf8")) as {
    id: string; purpose: string; transfer: boolean; expected: Record<Arm, { status: string; coverage: string[]; cost_microunits: number }>;
  }[];
  const tasks: CampaignTask[] = fixtures.map((f) => ({ id: f.id, purpose: f.purpose, transfer: f.transfer }));
  const expected = Object.fromEntries(fixtures.map((f) => [f.id, f.expected]));
  return runCampaign(tasks, expected);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  if (process.env.MEMORY_LIVE_CAMPAIGN !== "1") {
    console.log("Dry run (set MEMORY_LIVE_CAMPAIGN=1 with a real task list and provider credentials for a live campaign):");
    console.log(JSON.stringify(await dryRun(), null, 2));
  } else {
    console.error("Live campaigns need a real task list; this entrypoint only ships the dry-run fixture. See evals/campaign/README.md.");
    process.exit(1);
  }
}
