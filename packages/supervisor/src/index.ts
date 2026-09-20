// ABOUTME: Worker pool runtime: claims the next eligible job with a pool credential and hands it to the
// ABOUTME: composed handler for its process through the one assignment runner. Composition is validated first.
import { BACKGROUND_CONTEXT, withCancel, type Context } from "@earendil-works/pi-agent-core";
import type { IssuedWorkerCredential } from "../../../contracts/generated/host-response.js";
import type { Process } from "../../../contracts/generated/work-command.js";
import { HostClient, type HostCommands } from "../../host-client/src/client.js";
import type { Composition, ProcessModule } from "./composition.js";
import { activationModule } from "./activation.js";
import { consolidationModule } from "./consolidation.js";
import { evaluationModule } from "./evaluation.js";
import { formationModule } from "./formation.js";
import { investigationModule } from "./investigation.js";
import { maintenanceModule } from "./maintenance.js";
import { runAssignment, type ProcessHandler, type Work } from "./runner.js";

export type { ProcessHandler, ProcessRun, Work } from "./runner.js";
export { composeApplication, type Composition, type ProcessModule, type Services } from "./composition.js";
/** The modules this pool ships. A deployment composes a subset against its installed services. */
export const POOL_MODULES: ProcessModule[] = [investigationModule, formationModule, consolidationModule, evaluationModule, maintenanceModule, activationModule];
export type SupportedProcess = Extract<Process, "investigation" | "formation" | "consolidation" | "evaluation" | "maintenance" | "activation">;
export const SUPPORTED_PROCESSES: SupportedProcess[] = ["investigation", "formation", "consolidation", "evaluation", "maintenance", "activation"];
export type WorkClass = "interactive" | "deferred";
/** Capacity classes from the deployment profile (§20.2): interactive work answers a waiting task. */
export const CLASS_OF: Record<Process, WorkClass> = {
  activation: "interactive", investigation: "interactive",
  formation: "deferred", consolidation: "deferred", maintenance: "deferred", evaluation: "deferred",
};

export interface SupervisorOptions {
  /** Pool credential; used only to claim work. */
  host: HostCommands;
  /** Base URL for job-bound connections; unused when `connect` is supplied. */
  hostUrl?: string;
  /** Opens a Host connection with a job-bound credential. Defaults to `HostClient` at `hostUrl`. */
  connect?: (credential: IssuedWorkerCredential) => HostCommands;
  /** The validated composition; its handlers decide which processes are claimed. */
  composition: Composition;
  /** Slots per work class, or one number shared across all classes. */
  concurrency: number | Partial<Record<WorkClass, number>>;
  pollMs: number;
  leaseSeconds: number;
  /** After the context aborts, running work may finish for this long before it is cancelled. */
  drainMs?: number;
  sessionsDirectory: string;
  log?: (line: string) => void;
  /** Stop condition checked between polls; absent means run until the context aborts. */
  until?: () => Promise<boolean>;
}

/** Runs claim/dispatch slots until the context aborts or `until` reports done. On abort, claiming
 * stops at once; running work drains for `drainMs` and is then cancelled through the normal lifecycle. */
export async function runSupervisor(options: SupervisorOptions, context: Context = BACKGROUND_CONTEXT): Promise<void> {
  const log = options.log ?? ((line: string) => console.log(line));
  const registry = new Map<Process, ProcessHandler>(options.composition.handlers.map((h) => [h.process, h]));
  const connect = options.connect ?? ((credential: IssuedWorkerCredential) => {
    if (!options.hostUrl) throw new Error("hostUrl is required without a connect function");
    return new HostClient(options.hostUrl, credential.token);
  });
  const deps = { connect, sessionsDirectory: options.sessionsDirectory, leaseSeconds: options.leaseSeconds, composition: options.composition.identity, log };
  // Running work outlives the claim loop's context by the drain window.
  const { context: work, cancel: cancelWork } = withCancel(BACKGROUND_CONTEXT);
  let drainTimer: ReturnType<typeof setTimeout> | undefined;
  const onShutdown = () => {
    drainTimer = setTimeout(cancelWork, options.drainMs ?? 0);
    drainTimer.unref();
  };
  context.abortSignal?.addEventListener("abort", onShutdown, { once: true });
  const running = new Map<Promise<void>, WorkClass>();
  const processes = [...registry.keys()];
  const classes: WorkClass[] = [...new Set(processes.map((p) => CLASS_OF[p]))];
  const hasCapacity = (cls: WorkClass) =>
    typeof options.concurrency === "number"
      ? running.size < options.concurrency
      : [...running.values()].filter((c) => c === cls).length < (options.concurrency[cls] ?? 0);
  const sleep = (ms: number) =>
    new Promise<void>((done) => {
      const signal = context.abortSignal;
      const onAbort = () => { clearTimeout(timer); done(); };
      const timer = setTimeout(() => { signal?.removeEventListener("abort", onAbort); done(); }, ms);
      signal?.addEventListener("abort", onAbort, { once: true });
    });
  while (!context.abortSignal?.aborted) {
    if (options.until && running.size === 0 && (await options.until())) break;
    let claimedAny = false;
    for (const cls of classes) {
      const eligible = processes.filter((p) => CLASS_OF[p] === cls);
      while (hasCapacity(cls) && !context.abortSignal?.aborted) {
        const response = await options.host.request(
          { action: "claim_next", processes: eligible, lease_seconds: options.leaseSeconds },
          context.abortSignal,
        );
        if (response.kind === "idle") break;
        if (response.kind !== "work") throw new Error(`Host returned ${response.kind} to claim_next`);
        claimedAny = true;
        const claimed: Work = { assignment: response.assignment, credential: response.credential };
        const job = claimed.assignment.job;
        const handler = registry.get(job.spec.brief.process);
        if (!handler) throw new Error(`Host returned ${job.spec.brief.process} work this pool did not claim`);
        log(`claimed ${job.spec.brief.process} job ${job.id} (attempt ${job.attempt})`);
        const task: Promise<void> = runAssignment(claimed, handler, deps, work).finally(() => running.delete(task));
        running.set(task, cls);
      }
    }
    if (!claimedAny) {
      if (running.size === 0) log("idle");
      await Promise.race([sleep(options.pollMs), ...running.keys()]);
    }
  }
  if (running.size) log(`draining ${running.size} running job(s)`);
  await Promise.allSettled(running.keys());
  context.abortSignal?.removeEventListener("abort", onShutdown);
  clearTimeout(drainTimer);
  cancelWork();
}
