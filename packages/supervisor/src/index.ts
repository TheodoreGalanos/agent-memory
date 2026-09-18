// ABOUTME: Worker pool runtime: claims the next eligible job with a pool credential, drives it with
// ABOUTME: the job-bound credential the Host issued, and settles it. Investigation and formation only.
import { createHash } from "node:crypto";
import { join } from "node:path";
import { BACKGROUND_CONTEXT, type Context } from "@earendil-works/pi-agent-core";
import { NodeExecutionEnv } from "@earendil-works/pi-agent-core/node";
import type { Model, Api, Models } from "@earendil-works/pi-ai";
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type { HostResponse, IssuedWorkerCredential } from "../../../contracts/generated/host-response.js";
import type { Process } from "../../../contracts/generated/work-command.js";
import type { WorkResult } from "../../../contracts/generated/work-result.js";
import type { FormationResult } from "../../../contracts/generated/formation-result.js";
import { HostClient, type HostCommands } from "../../pi-worker/src/host-client.js";
import { openLocalSession } from "../../pi-worker/src/local-session.js";
import { attachAssignedWorker } from "../../pi-worker/src/assigned-worker.js";
import type { JudgementProvider } from "../../judgement/src/providers.js";
import type { JudgementOptions } from "../../judgement/src/runtime.js";
import { fenceFor } from "../../judgement/src/packet.js";
import { runFormation } from "../../formation/src/index.js";
import { constrainToWorkResult, deliverInputMemories, taskSystemPrompt } from "./task.js";
import { observeProvider } from "./trace.js";

export type SupportedProcess = Extract<Process, "investigation" | "formation">;
export const SUPPORTED_PROCESSES: SupportedProcess[] = ["investigation", "formation"];
export type WorkClass = "interactive" | "deferred";
/** Capacity classes from the deployment profile (§20.2): interactive work answers a waiting task. */
export const CLASS_OF: Record<Process, WorkClass> = {
  activation: "interactive", investigation: "interactive",
  formation: "deferred", consolidation: "deferred", maintenance: "deferred", evaluation: "deferred",
};

export interface SupervisorOptions {
  /** Pool credential; used only to claim work. */
  host: HostCommands;
  hostUrl: string;
  processes: SupportedProcess[];
  /** Slots per work class, or one shared number. Interactive and deferred capacity stay separate. */
  concurrency: number | Partial<Record<WorkClass, number>>;
  pollMs: number;
  leaseSeconds: number;
  sessionsDirectory: string;
  models: Models;
  taskModel: Model<Api>;
  maxPayloadBytes: number;
  /** Cost reservation per task provider call, in microunits. Settles to observed usage. */
  taskReservationCost?: number;
  judgement: {
    reference: JudgementProvider;
    jev?: JudgementProvider;
    mode?: JudgementOptions["mode"];
    maximum: JudgementProvider["maximum"];
  };
  log?: (line: string) => void;
  /** Stop condition checked between polls; absent means run until the context aborts. */
  until?: () => Promise<boolean>;
}

export interface Work {
  assignment: Assignment;
  credential: IssuedWorkerCredential;
}

/** Runs claim/dispatch slots until the context aborts or `until` reports done. */
export async function runSupervisor(options: SupervisorOptions, context: Context = BACKGROUND_CONTEXT): Promise<void> {
  const log = options.log ?? ((line: string) => console.log(line));
  if (options.processes.some((p) => !SUPPORTED_PROCESSES.includes(p)))
    throw new Error(`This pool supports ${SUPPORTED_PROCESSES.join(", ")} only`);
  const running = new Map<Promise<void>, WorkClass>();
  const classes: WorkClass[] = [...new Set(options.processes.map((p) => CLASS_OF[p]))];
  const slots = (cls: WorkClass) =>
    typeof options.concurrency === "number" ? options.concurrency : (options.concurrency[cls] ?? 0);
  const active = (cls: WorkClass) => [...running.values()].filter((c) => c === cls).length;
  const sleep = (ms: number) =>
    new Promise<void>((done) => {
      const timer = setTimeout(done, ms);
      context.abortSignal?.addEventListener("abort", () => { clearTimeout(timer); done(); }, { once: true });
    });
  while (!context.abortSignal?.aborted) {
    if (options.until && running.size === 0 && (await options.until())) break;
    let claimed = false;
    for (const cls of classes) {
      const processes = options.processes.filter((p) => CLASS_OF[p] === cls);
      while (active(cls) < slots(cls) && !context.abortSignal?.aborted) {
        const response = await options.host.request(
          { action: "claim_next", processes, lease_seconds: options.leaseSeconds },
          context.abortSignal,
        );
        if (response.kind === "idle") break;
        if (response.kind !== "work") throw new Error(`Host returned ${response.kind} to claim_next`);
        claimed = true;
        const work: Work = { assignment: response.assignment, credential: response.credential };
        const job = work.assignment.job;
        log(`claimed ${job.spec.brief.process} job ${job.id} (attempt ${job.attempt})`);
        const task: Promise<void> = dispatch(work, options, context, log)
          .catch(async (error: unknown) => {
            log(`job ${job.id} failed: ${error instanceof Error ? error.message : String(error)}`);
            await release(work, options, context);
          })
          .finally(() => running.delete(task));
        running.set(task, cls);
      }
    }
    if (!claimed) {
      if (running.size === 0) log("idle");
      await Promise.race([sleep(options.pollMs), ...running.keys()]);
    }
  }
  await Promise.allSettled(running.keys());
}

/** A failed attempt shortens its lease so coordinator recovery can requeue it promptly. */
async function release(work: Work, options: SupervisorOptions, context: Context) {
  try {
    const host = new HostClient(options.hostUrl, work.credential.token);
    await host.request({ action: "renew", fence: fenceFor(work.assignment), lease_seconds: 1 }, context.abortSignal);
  } catch {
    /* The lease expires on its own if the fence is already superseded. */
  }
}

async function dispatch(work: Work, options: SupervisorOptions, context: Context, log: (line: string) => void) {
  const process = work.assignment.job.spec.brief.process;
  if (process === "investigation") return runInvestigation(work, options, context, log);
  if (process === "formation") return runFormationJob(work, options, context, log);
  throw new Error(`Unsupported process ${process}`);
}

async function runInvestigation(work: Work, options: SupervisorOptions, context: Context, log: (line: string) => void) {
  const { assignment } = work;
  const brief = assignment.job.spec.brief;
  const host = new HostClient(options.hostUrl, work.credential.token);
  const local = await openLocalSession(join(options.sessionsDirectory, "sessions"), assignment.job.session_id);
  try {
    const model = options.taskModel;
    const profile = {
      ...brief.profile,
      maxPayloadBytes: options.maxPayloadBytes,
      maxOutputTokens: model.maxTokens,
      providerReservation: {
        tokens: options.maxPayloadBytes + model.maxTokens,
        provider_calls: 1,
        cost_microunits: options.taskReservationCost ?? 100_000,
        output_bytes: 0,
        sandbox_cpu_ms: 0,
        sandbox_time_ms: 0,
      },
    };
    const worker = await attachAssignedWorker(assignment, host, profile, {
      session: local.session,
      models: options.models,
      model,
      env: new NodeExecutionEnv({ cwd: options.sessionsDirectory }),
      tools: [],
      systemPrompt: taskSystemPrompt(brief),
    }, context);
    try {
      if (model.api === "azure-openai-responses")
        worker.harness.hooks.on("before_payload", async ({ payload }) => ({ payload: constrainToWorkResult(payload) }));
      if (brief.inputs.memories.length) {
        const delivered = await host.request(
          { action: "current_memories", fence: fenceFor(assignment), references: brief.inputs.memories },
          context.abortSignal,
        );
        if (delivered.kind !== "memories") throw new Error("Host did not return current memories");
        try {
          await worker.workspace.checkpoint(deliverInputMemories(await worker.workspace.state(), delivered.memories));
        } catch (error) {
          // A granted input that was revised or withdrawn blocks this brief; it is not a transient failure.
          const reason = error instanceof Error ? error.message : String(error);
          await host.request({ action: "start", fence: fenceFor(assignment) }, context.abortSignal);
          await host.request({
            action: "complete", request_id: windowId(assignment.job.id, { source_id: "blocked", revision: String(assignment.job.attempt) }), fence: fenceFor(assignment),
            result: {
              schema_version: "1", status: "blocked", examined_scope: brief.scope, inputs: brief.inputs, findings: [],
              coverage: { examined: [], unexamined: brief.inputs.memories.map((m) => `${m.label} r${m.revision}`) },
              unresolved_work: [`${reason}; resubmit the brief with the current reference`],
              proposed_changes: [], child_outputs: [], known_effects: [],
              usage: { status: "known", input_tokens: 0, output_tokens: 0, cost: null },
            },
          }, context.abortSignal);
          log(`job ${assignment.job.id} blocked: ${reason}`);
          return;
        }
      }
      const outcome = await worker.run();
      log(`job ${assignment.job.id} ${outcome.kind}${outcome.kind === "settled" ? ` (${outcome.job.state})` : ""}`);
    } finally {
      await worker.close();
    }
  } finally {
    await local.close();
  }
}

/** One window per input source, with a window id derived from the job and source so a retried
 * attempt resumes the same capture instead of paying again. */
function windowId(jobId: string, source: { source_id: string; revision: string }): string {
  const digest = createHash("sha256").update(`${jobId}:${source.source_id}:${source.revision}`).digest();
  digest[6] = (digest[6] & 0x0f) | 0x80;
  digest[8] = (digest[8] & 0x3f) | 0x80;
  const hex = digest.subarray(0, 16).toString("hex");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20, 32)}`;
}

async function runFormationJob(work: Work, options: SupervisorOptions, context: Context, log: (line: string) => void) {
  const { assignment } = work;
  const brief = assignment.job.spec.brief;
  const host = new HostClient(options.hostUrl, work.credential.token);
  const fence = fenceFor(assignment);
  if (assignment.job.state !== "running") await host.request({ action: "start", fence }, context.abortSignal);
  const local = await openLocalSession(join(options.sessionsDirectory, "sessions"), assignment.job.session_id);
  try {
    const judgement: JudgementOptions = {
      reference: observeProvider(options.judgement.reference, "reference", log),
      jev: options.judgement.jev ? observeProvider(options.judgement.jev, "shadow", log) : undefined,
      mode: options.judgement.mode,
      maxAttempts: 1,
      disclosure: {
        policy: brief.disclosure_policy,
        scope: brief.scope,
        providers: [options.judgement.reference.id, ...(options.judgement.jev ? [options.judgement.jev.id] : [])],
      },
    };
    // The brief's purpose names the formation operation: jobs with the same purpose continue a
    // source's cursor, while a new purpose explicitly reinterprets the source (WP09).
    const operation = brief.purpose.trim().slice(0, 200) || "capture";
    const results: FormationResult[] = [];
    for (const source of brief.inputs.sources) {
      await host.request({ action: "renew", fence, lease_seconds: options.leaseSeconds }, context.abortSignal);
      const result = await runFormation(host, assignment, local.session, { id: windowId(assignment.job.id, source), source, operation, limit: 32 }, judgement, context);
      results.push(result);
      log(`job ${assignment.job.id} source ${source.source_id}@${source.revision}: retained ${result.records.length}, deferred ${result.deferred.length}`);
    }
    const coverage = { examined: results.flatMap((r) => r.coverage.examined), unexamined: results.flatMap((r) => r.coverage.unexamined) };
    // Unexamined material is itself unresolved work: a partial result must say what remains.
    const unresolved = [...new Set([...results.flatMap((r) => r.unresolved), ...coverage.unexamined.map((item) => `Unexamined: ${item}`)])];
    const usage = results.at(-1)?.usage ?? { status: "unknown" as const, input_tokens: null, output_tokens: null, cost: null };
    const completion: WorkResult = {
      schema_version: "1",
      status: unresolved.length || coverage.unexamined.length ? "partial" : "complete",
      examined_scope: brief.scope,
      inputs: brief.inputs,
      findings: [],
      coverage,
      unresolved_work: unresolved,
      proposed_changes: [],
      child_outputs: [],
      known_effects: [],
      usage,
    };
    const committed: HostResponse = await host.request(
      { action: "complete", request_id: windowId(assignment.job.id, { source_id: "complete", revision: String(assignment.job.attempt) }), fence, result: completion },
      context.abortSignal,
    );
    log(`job ${assignment.job.id} ${committed.kind === "job" ? committed.job.state : committed.kind}`);
  } finally {
    await local.close();
  }
}
