// ABOUTME: Unit tests for the assignment runner against a scripted Host: lease heartbeat, cancellation
// ABOUTME: delivery, failure release, cleanup, shared concurrency and poll-loop listener hygiene.
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { randomUUID } from "node:crypto";
import { getEventListeners } from "node:events";
import { BACKGROUND_CONTEXT, withAbortSignal } from "@earendil-works/pi-agent-core";
import { afterEach, beforeEach, expect, it } from "vitest";
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type { HostRequest } from "../../../contracts/generated/host-request.js";
import type { HostResponse, IssuedWorkerCredential } from "../../../contracts/generated/host-response.js";
import type { Process } from "../../../contracts/generated/work-command.js";
import { input } from "../../pi-worker/test/fixtures.js";
import { keepLease, runAssignment, type ProcessHandler } from "../src/runner.js";
import { composeApplication } from "../src/composition.js";
import { runSupervisor } from "../src/index.js";

/** A composition of one test module carrying the given handlers. */
const composed = (handlers: ProcessHandler[]) =>
  composeApplication({}, [{ declaration: { id: "test", version: "1", requires: [], operations: handlers.map((h) => h.process) }, attach: () => handlers }]);

const seconds = (n: number) => new Date(Date.now() + n * 1000).toISOString();
const sleep = (ms: number) => new Promise<void>((done) => setTimeout(done, ms));

function assignmentFor(process: Process, sessionId = randomUUID()): Assignment {
  const brief = input("before-correction").command.payload;
  return {
    epoch: 1,
    owner_id: randomUUID(),
    expires_at: seconds(30),
    job: {
      id: randomUUID(), root_id: randomUUID(), depth: 0, attempt: 1, session_id: sessionId, operation_id: randomUUID(),
      state: "leased", deadline: seconds(120), cancel_requested: false,
      spec: { brief: { ...brief, process }, max_attempts: 3, retain_until: seconds(600) },
    },
  };
}
const credential = (assignment: Assignment): IssuedWorkerCredential => ({
  actor_id: randomUUID(), expires_at: assignment.job.deadline, job_id: assignment.job.id, scope: assignment.job.spec.brief.scope, token: "t".repeat(40),
});

/** A Host that hands out the queued assignments once and answers lease commands from shared state. */
function scriptedHost(queue: Assignment[] = []) {
  const calls: HostRequest[] = [];
  const state = { renewFails: false, cancel: new Set<string>() };
  const host = {
    calls,
    state,
    async request(command: HostRequest): Promise<HostResponse> {
      calls.push(command);
      if (command.action === "claim_next") {
        const index = queue.findIndex((a) => command.processes.includes(a.job.spec.brief.process));
        if (index < 0) return { kind: "idle" };
        const [assignment] = queue.splice(index, 1);
        return { kind: "work", assignment, credential: credential(assignment) };
      }
      if (command.action === "renew") {
        if (state.renewFails) throw new Error("lease lost");
        const assignment = [...handedOut.values()].find((a) => a.job.id === command.fence.job_id)!;
        const renewed = { ...assignment, expires_at: seconds(command.lease_seconds), job: { ...assignment.job, cancel_requested: state.cancel.has(assignment.job.id) } };
        return { kind: "assignment", assignment: renewed };
      }
      if (command.action === "acknowledge_cancellation") {
        const assignment = [...handedOut.values()].find((a) => a.job.id === command.fence.job_id)!;
        return { kind: "job", job: { ...assignment.job, state: "cancelled", cancel_requested: true } };
      }
      throw new Error(`Unscripted host command ${command.action}`);
    },
  };
  const handedOut = new Map<string, Assignment>();
  for (const a of queue) handedOut.set(a.job.id, a);
  return { ...host, track: (a: Assignment) => handedOut.set(a.job.id, a) };
}

let base: string;
beforeEach(async () => { base = await mkdtemp(join(tmpdir(), "memory-runner-")); });
afterEach(async () => { await rm(base, { recursive: true, force: true }); });

it("the lease keeper renews on a heartbeat, publishes the renewed assignment and delivers cancellation once", async () => {
  const assignment = assignmentFor("formation");
  const host = scriptedHost(); host.track(assignment);
  const seen: string[] = [];
  let cancellations = 0;
  const lease = keepLease(host, assignment, 1, BACKGROUND_CONTEXT);
  lease.onRenew((a) => { seen.push(a.expires_at); });
  lease.onCancel(() => { cancellations++; });
  await sleep(800);
  expect(host.calls.filter((c) => c.action === "renew").length).toBeGreaterThanOrEqual(2);
  expect(seen.length).toBeGreaterThanOrEqual(2);
  expect(lease.assignment.expires_at).toBe(seen.at(-1));
  expect(lease.cancelled).toBe(false);
  host.state.cancel.add(assignment.job.id);
  await sleep(800);
  expect(lease.cancelled).toBe(true);
  expect(cancellations).toBe(1);
  // Losing ownership is different from cancellation: the keeper's context aborts and stays aborted.
  expect(lease.context.abortSignal?.aborted).toBe(false);
  await lease.stop();
  const renewals = host.calls.filter((c) => c.action === "renew").length;
  await sleep(500);
  expect(host.calls.filter((c) => c.action === "renew").length).toBe(renewals);
});

it("the lease keeper aborts its context when renewal fails and reports the loss", async () => {
  const assignment = assignmentFor("formation");
  const host = scriptedHost(); host.track(assignment);
  const lease = keepLease(host, assignment, 1, BACKGROUND_CONTEXT);
  host.state.renewFails = true;
  await sleep(700);
  expect(lease.context.abortSignal?.aborted).toBe(true);
  expect(lease.lost?.message).toBe("lease lost");
  await lease.stop();
});

it("the runner opens the job session, runs the handler with a job-bound host, and releases a failed attempt", async () => {
  const assignment = assignmentFor("formation");
  const host = scriptedHost(); host.track(assignment);
  const connected: string[] = [];
  let sessionId: string | undefined;
  const handler: ProcessHandler = {
    process: "formation",
    async run(run) {
      sessionId = run.session.metadata.id;
      expect(run.composition).toBe("test@1#0");
      expect(run.lease.fence).toEqual({ job_id: assignment.job.id, owner_id: assignment.owner_id, epoch: 1 });
      throw new Error("handler failed");
    },
  };
  const log: string[] = [];
  await runAssignment(
    { assignment, credential: credential(assignment) },
    handler,
    { connect: (c) => { connected.push(c.token); return host; }, sessionsDirectory: base, leaseSeconds: 60, composition: "test@1#0", log: (l) => log.push(l) },
    BACKGROUND_CONTEXT,
  );
  expect(connected).toEqual(["t".repeat(40)]);
  expect(sessionId).toBe(assignment.job.session_id);
  expect(log.some((l) => l.includes("failed") && l.includes("handler failed"))).toBe(true);
  // A failed attempt shortens its lease so recovery requeues it promptly.
  expect(host.calls.at(-1)).toMatchObject({ action: "renew", lease_seconds: 1 });
});

it("the runner acknowledges cancellation when the handler is interrupted by a cancel request", async () => {
  const assignment = assignmentFor("formation");
  const host = scriptedHost(); host.track(assignment);
  const handler: ProcessHandler = {
    process: "formation",
    async run(run) {
      host.state.cancel.add(assignment.job.id);
      // The run context aborts on cancellation; long work simply awaits it like any host call would.
      await new Promise<void>((_, fail) => run.context.abortSignal?.addEventListener("abort", () => fail(new Error("aborted")), { once: true }));
    },
  };
  const log: string[] = [];
  await runAssignment(
    { assignment, credential: credential(assignment) },
    handler,
    { connect: () => host, sessionsDirectory: base, leaseSeconds: 1, composition: "test@1#0", log: (l) => log.push(l) },
    BACKGROUND_CONTEXT,
  );
  const acknowledged = host.calls.findIndex((c) => c.action === "acknowledge_cancellation");
  expect(acknowledged).toBeGreaterThan(-1);
  // A cancelled job is settled, not released as a failed attempt.
  expect(host.calls.slice(acknowledged + 1).some((c) => c.action === "renew" && c.lease_seconds === 1)).toBe(false);
  expect(log.some((l) => l.includes("cancelled"))).toBe(true);
});

it("a single concurrency number is a shared cap across work classes", async () => {
  const queue = [assignmentFor("investigation"), assignmentFor("formation"), assignmentFor("investigation"), assignmentFor("formation")];
  const host = scriptedHost(queue);
  let inFlight = 0, peak = 0, finished = 0;
  const handler = (process: Process): ProcessHandler => ({
    process,
    async run() { inFlight++; peak = Math.max(peak, inFlight); await sleep(60); inFlight--; finished++; },
  });
  await runSupervisor({
    host, connect: () => host, concurrency: 1, pollMs: 10, leaseSeconds: 60,
    sessionsDirectory: base, composition: composed([handler("investigation"), handler("formation")]), log: () => {},
    until: async () => finished === 4,
  });
  expect(finished).toBe(4);
  expect(peak).toBe(1);
});

it("per-class concurrency keeps interactive and deferred slots separate", async () => {
  const queue = [assignmentFor("investigation"), assignmentFor("formation")];
  const host = scriptedHost(queue);
  let inFlight = 0, peak = 0, finished = 0;
  // Each handler holds its slot until the other class has also started (or a timeout proves it never did).
  let release!: () => void;
  const bothStarted = new Promise<void>((done) => { release = done; });
  const handler = (process: Process): ProcessHandler => ({
    process,
    async run() {
      inFlight++; peak = Math.max(peak, inFlight);
      if (inFlight === 2) release();
      await Promise.race([bothStarted, sleep(2_000)]);
      inFlight--; finished++;
    },
  });
  await runSupervisor({
    host, connect: () => host, concurrency: { interactive: 1, deferred: 1 }, pollMs: 10, leaseSeconds: 60,
    sessionsDirectory: base, composition: composed([handler("investigation"), handler("formation")]), log: () => {},
    until: async () => finished === 2,
  });
  expect(peak).toBe(2);
});

it("the poll loop does not accumulate abort listeners while idle", async () => {
  const host = scriptedHost();
  const controller = new AbortController();
  let polls = 0;
  const run = runSupervisor({
    host, connect: () => host, concurrency: 1, pollMs: 5, leaseSeconds: 60,
    sessionsDirectory: base, composition: composed([{ process: "investigation", async run() {} }]), log: () => {},
    until: async () => ++polls > 40,
  }, withAbortSignal(controller.signal, BACKGROUND_CONTEXT));
  await run;
  expect(polls).toBeGreaterThan(40);
  expect(getEventListeners(controller.signal, "abort").length).toBeLessThanOrEqual(1);
});

it("shutdown drains running work within the drain window instead of cancelling it", async () => {
  const assignment = assignmentFor("investigation");
  const host = scriptedHost([assignment]);
  const controller = new AbortController();
  const log: string[] = [];
  let finished = false;
  const handler: ProcessHandler = {
    process: "investigation",
    async run(run) {
      controller.abort();
      await sleep(150);
      if (run.context.abortSignal?.aborted) throw new Error("aborted during drain");
      finished = true;
    },
  };
  await runSupervisor({
    host, connect: () => host, concurrency: 1, pollMs: 10, leaseSeconds: 60, drainMs: 2_000,
    sessionsDirectory: base, composition: composed([handler]), log: (l) => log.push(l),
  }, withAbortSignal(controller.signal, BACKGROUND_CONTEXT));
  expect(finished).toBe(true);
  expect(log.filter((l) => l.includes("failed"))).toEqual([]);
  // Nothing new is claimed once shutdown begins.
  expect(host.calls.filter((c) => c.action === "claim_next").length).toBe(1);
});

it("shutdown cancels running work once the drain window passes", async () => {
  const assignment = assignmentFor("investigation");
  const host = scriptedHost([assignment]);
  const controller = new AbortController();
  const log: string[] = [];
  const handler: ProcessHandler = {
    process: "investigation",
    async run(run) {
      controller.abort();
      await new Promise<void>((_, fail) => run.context.abortSignal?.addEventListener("abort", () => fail(new Error("cancelled by shutdown")), { once: true }));
    },
  };
  await runSupervisor({
    host, connect: () => host, concurrency: 1, pollMs: 10, leaseSeconds: 60, drainMs: 100,
    sessionsDirectory: base, composition: composed([handler]), log: (l) => log.push(l),
  }, withAbortSignal(controller.signal, BACKGROUND_CONTEXT));
  expect(log.some((l) => l.includes("failed") && l.includes("cancelled by shutdown"))).toBe(true);
  expect(host.calls.at(-1)).toMatchObject({ action: "renew", lease_seconds: 1 });
});
