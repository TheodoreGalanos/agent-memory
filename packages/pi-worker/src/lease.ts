// ABOUTME: The one lease keeper for an assignment: renews ownership on a heartbeat, publishes each renewed
// ABOUTME: assignment to listeners, delivers a cancel request once, and aborts its context when ownership is lost.
import { withCancel, type Context } from "@earendil-works/pi-agent-core";
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type { Fence } from "../../../contracts/generated/host-request.js";
import type { HostCommands } from "../../host-client/src/client.js";
import { fenceFor } from "../../host-client/src/fence.js";

export type Unsubscribe = () => void;

/** Ownership of one assignment for the duration of an attempt. Losing the lease aborts `context`;
 * a cancel request is delivered separately so drivers can wind down their own state machines. */
export interface AssignmentLease {
  /** The assignment as last confirmed by the Host. */
  readonly assignment: Assignment;
  readonly fence: Fence;
  /** Aborts when ownership is lost or the parent stops; never on cancellation alone. */
  readonly context: Context;
  readonly cancelled: boolean;
  readonly lost: Error | undefined;
  /** Renews now. Listeners run before this resolves; a listener failure counts as a lost lease. */
  renew(): Promise<Assignment>;
  onRenew(listener: (assignment: Assignment) => void | Promise<void>): Unsubscribe;
  onCancel(listener: () => void | Promise<void>): Unsubscribe;
  stop(): Promise<void>;
}

/** Renews the lease on a heartbeat well inside `leaseSeconds` until stopped or lost. */
export function keepLease(host: HostCommands, initial: Assignment, leaseSeconds: number, parent: Context): AssignmentLease {
  const fence = fenceFor(initial);
  const { context, cancel } = withCancel(parent);
  const heartbeatMs = Math.max(200, Math.min(5_000, (leaseSeconds * 1000) / 3));
  const renewListeners = new Set<(assignment: Assignment) => void | Promise<void>>();
  const cancelListeners = new Set<() => void | Promise<void>>();
  let assignment = initial;
  let cancelled = initial.job.cancel_requested;
  let lost: Error | undefined;
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let inflight: Promise<unknown> = Promise.resolve();

  async function observe(renewed: Assignment) {
    assignment = renewed;
    for (const listener of renewListeners) await listener(renewed);
    if (renewed.job.cancel_requested && !cancelled) {
      cancelled = true;
      for (const listener of cancelListeners) await listener();
    }
  }
  async function renew(): Promise<Assignment> {
    if (lost) throw lost;
    if (stopped) throw new Error("Assignment lease keeper is stopped");
    try {
      const response = await host.request({ action: "renew", fence, lease_seconds: leaseSeconds }, context.abortSignal);
      if (response.kind !== "assignment") throw new Error("Host did not renew assignment");
      await observe(response.assignment);
      return response.assignment;
    } catch (error) {
      lost = error instanceof Error ? error : new Error("Assignment renewal failed");
      cancel();
      throw lost;
    }
  }
  function schedule() {
    if (stopped || lost) return;
    timer = setTimeout(() => {
      inflight = renew().then(schedule, () => {});
    }, heartbeatMs);
    timer.unref();
  }
  schedule();

  return {
    get assignment() { return assignment; },
    fence,
    context,
    get cancelled() { return cancelled; },
    get lost() { return lost; },
    renew,
    onRenew(listener) {
      renewListeners.add(listener);
      return () => renewListeners.delete(listener);
    },
    onCancel(listener) {
      cancelListeners.add(listener);
      // A cancel already on record is delivered to a late subscriber once, asynchronously.
      if (cancelled) queueMicrotask(() => { if (cancelListeners.has(listener)) void listener(); });
      return () => cancelListeners.delete(listener);
    },
    async stop() {
      stopped = true;
      clearTimeout(timer);
      await inflight;
      cancel();
    },
  };
}
