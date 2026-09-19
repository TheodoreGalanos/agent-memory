// ABOUTME: The one assignment runner: connects with the job-bound credential, opens the job's Pi session,
// ABOUTME: keeps the lease renewed, delivers cancellation, and settles or releases the attempt. Handlers only do process work.
import { join } from "node:path";
import { withCancel, type Context, type Session } from "@earendil-works/pi-agent-core";
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type { IssuedWorkerCredential } from "../../../contracts/generated/host-response.js";
import type { Process } from "../../../contracts/generated/work-command.js";
import type { HostCommands } from "../../pi-worker/src/host-client.js";
import { keepLease, type AssignmentLease } from "../../pi-worker/src/lease.js";
import { openLocalSession } from "../../pi-worker/src/local-session.js";

export { keepLease, type AssignmentLease } from "../../pi-worker/src/lease.js";

export interface Work {
  assignment: Assignment;
  credential: IssuedWorkerCredential;
}

/** What a process handler receives: the job-bound Host, the job's Pi session, and a context that
 * aborts when the work should stop for any reason (lease lost, cancelled, or pool shutting down). */
export interface ProcessRun {
  host: HostCommands;
  lease: AssignmentLease;
  session: Session;
  sessionsDirectory: string;
  context: Context;
  log(line: string): void;
}
export interface ProcessHandler<P extends Process = Process> {
  process: P;
  run(run: ProcessRun): Promise<void>;
}

export interface RunnerDeps {
  /** Opens a Host connection with the job-bound credential the pool was issued. */
  connect(credential: IssuedWorkerCredential): HostCommands;
  sessionsDirectory: string;
  leaseSeconds: number;
  log(line: string): void;
}

/** Runs one claimed assignment to a settled, cancelled or released outcome. Never throws for handler failures. */
export async function runAssignment(work: Work, handler: ProcessHandler, deps: RunnerDeps, parent: Context): Promise<void> {
  const { assignment } = work;
  const job = assignment.job;
  const host = deps.connect(work.credential);
  const lease = keepLease(host, assignment, deps.leaseSeconds, parent);
  const { context, cancel } = withCancel(lease.context);
  lease.onCancel(cancel);
  let local: Awaited<ReturnType<typeof openLocalSession>> | undefined;
  try {
    local = await openLocalSession(join(deps.sessionsDirectory, "sessions"), job.session_id);
    await handler.run({ host, lease, session: local.session, sessionsDirectory: deps.sessionsDirectory, context, log: deps.log });
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    if (lease.cancelled && !lease.lost) {
      await acknowledge(host, lease, deps.log);
    } else {
      deps.log(`job ${job.id} failed: ${reason}`);
      await release(host, lease);
    }
  } finally {
    cancel();
    await lease.stop();
    await local?.close();
  }
}

async function acknowledge(host: HostCommands, lease: AssignmentLease, log: (line: string) => void) {
  try {
    await host.request({ action: "acknowledge_cancellation", fence: lease.fence });
    log(`job ${lease.fence.job_id} cancelled`);
  } catch (error) {
    // Unresolved effects keep the job open; the Host's recovery path settles it.
    log(`job ${lease.fence.job_id} cancelled; acknowledgement failed: ${error instanceof Error ? error.message : String(error)}`);
  }
}

/** A failed attempt shortens its lease so coordinator recovery can requeue it promptly. */
async function release(host: HostCommands, lease: AssignmentLease) {
  try {
    await host.request({ action: "renew", fence: lease.fence, lease_seconds: 1 });
  } catch {
    /* The lease expires on its own if the fence is already superseded. */
  }
}
