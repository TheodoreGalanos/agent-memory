// ABOUTME: Worker pool runtime: claims the next eligible job with a pool credential and hands it to the
// ABOUTME: registered handler for its process through the one assignment runner. Handlers are explicit.
import { BACKGROUND_CONTEXT, type Context } from "@earendil-works/pi-agent-core";
import type { IssuedWorkerCredential } from "../../../contracts/generated/host-response.js";
import type { Process } from "../../../contracts/generated/work-command.js";
import { HostClient, type HostCommands } from "../../pi-worker/src/host-client.js";
import { formationHandler, type FormationDeps } from "./formation.js";
import { investigationHandler, type InvestigationDeps } from "./investigation.js";
import { runAssignment, type ProcessHandler, type Work } from "./runner.js";

export type { ProcessHandler, ProcessRun, Work } from "./runner.js";
export type SupportedProcess = Extract<Process, "investigation" | "formation">;
export const SUPPORTED_PROCESSES: SupportedProcess[] = ["investigation", "formation"];
export type WorkClass = "interactive" | "deferred";
/** Capacity classes from the deployment profile (§20.2): interactive work answers a waiting task. */
export const CLASS_OF: Record<Process, WorkClass> = {
  activation: "interactive", investigation: "interactive",
  formation: "deferred", consolidation: "deferred", maintenance: "deferred", evaluation: "deferred",
};

/** The handlers this pool ships: investigation on the task model, formation on the judgement providers. */
export function processHandlers(deps: InvestigationDeps & FormationDeps): ProcessHandler[] {
  return [investigationHandler(deps), formationHandler(deps)];
}

export interface SupervisorOptions {
  /** Pool credential; used only to claim work. */
  host: HostCommands;
  /** Base URL for job-bound connections; unused when `connect` is supplied. */
  hostUrl?: string;
  /** Opens a Host connection with a job-bound credential. Defaults to `HostClient` at `hostUrl`. */
  connect?: (credential: IssuedWorkerCredential) => HostCommands;
  /** One handler per process this pool claims. */
  handlers: ProcessHandler[];
  /** Slots per work class, or one number shared across all classes. */
  concurrency: number | Partial<Record<WorkClass, number>>;
  pollMs: number;
  leaseSeconds: number;
  sessionsDirectory: string;
  log?: (line: string) => void;
  /** Stop condition checked between polls; absent means run until the context aborts. */
  until?: () => Promise<boolean>;
}

/** Validates the registration and returns the handler for each claimed process. */
export function registerHandlers(handlers: ProcessHandler[]): Map<Process, ProcessHandler> {
  if (!handlers.length) throw new Error("The pool needs at least one process handler");
  const registry = new Map<Process, ProcessHandler>();
  for (const handler of handlers) {
    if (registry.has(handler.process)) throw new Error(`Process ${handler.process} has more than one handler`);
    registry.set(handler.process, handler);
  }
  return registry;
}

/** Runs claim/dispatch slots until the context aborts or `until` reports done. */
export async function runSupervisor(options: SupervisorOptions, context: Context = BACKGROUND_CONTEXT): Promise<void> {
  const log = options.log ?? ((line: string) => console.log(line));
  const registry = registerHandlers(options.handlers);
  const connect = options.connect ?? ((credential: IssuedWorkerCredential) => {
    if (!options.hostUrl) throw new Error("hostUrl is required without a connect function");
    return new HostClient(options.hostUrl, credential.token);
  });
  const deps = { connect, sessionsDirectory: options.sessionsDirectory, leaseSeconds: options.leaseSeconds, log };
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
    let claimed = false;
    for (const cls of classes) {
      const eligible = processes.filter((p) => CLASS_OF[p] === cls);
      while (hasCapacity(cls) && !context.abortSignal?.aborted) {
        const response = await options.host.request(
          { action: "claim_next", processes: eligible, lease_seconds: options.leaseSeconds },
          context.abortSignal,
        );
        if (response.kind === "idle") break;
        if (response.kind !== "work") throw new Error(`Host returned ${response.kind} to claim_next`);
        claimed = true;
        const work: Work = { assignment: response.assignment, credential: response.credential };
        const job = work.assignment.job;
        const handler = registry.get(job.spec.brief.process);
        if (!handler) throw new Error(`Host returned ${job.spec.brief.process} work this pool did not claim`);
        log(`claimed ${job.spec.brief.process} job ${job.id} (attempt ${job.attempt})`);
        const task: Promise<void> = runAssignment(work, handler, deps, context).finally(() => running.delete(task));
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
