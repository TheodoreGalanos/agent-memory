// ABOUTME: Tests for the Host scheduler's typed `WorkBrief.scheduled` fallback: maintenance derives its
// ABOUTME: manifest directly from it with no artifact round trip, idempotently across retried attempts.
import { expect, it, vi } from "vitest";
import { BACKGROUND_CONTEXT as context } from "@earendil-works/pi-agent-core";
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type { HostResponse } from "../../../contracts/generated/host-response.js";
import type { MaintenanceRequest } from "../../../contracts/generated/maintenance-review.js";
import { maintenanceInput } from "../src/maintenance.js";
import { evaluationHandler } from "../src/evaluation.js";
import type { ProcessRun } from "../src/runner.js";

const request: MaintenanceRequest = {
  before: { memory_id: "01a0bf4c-f7e6-7694-b6b5-50f9604cdc8a", revision: 1, label: "Beam clearance" },
  after: null,
  removed_sources: [],
  candidates: [{ memory_id: "01a0bf4c-f7e7-7576-ae1c-2f7004663a37", revision: 1, label: "Dependent note" }],
  reason: "Propagating a correction to its dependents",
};

function assignment(over: { attempt?: number; scheduled?: unknown; artifacts?: string[] }): Assignment {
  return {
    job: {
      id: "01a0bf4c-f7e5-7000-8000-000000000000",
      attempt: over.attempt ?? 1,
      spec: {
        brief: {
          process: "maintenance",
          inputs: { memories: [request.before], sources: [], artifacts: over.artifacts ?? [] },
          scheduled: over.scheduled,
        },
      },
    },
  } as unknown as Assignment;
}

it("derives a correction manifest directly from a scheduled maintenance_correction request, with no artifact read", async () => {
  const a = assignment({ scheduled: { kind: "maintenance_correction", request } });
  const neverCalled = { request: () => { throw new Error("must not read an artifact when a scheduled request is present"); } };
  const manifest = await maintenanceInput(neverCalled as never, a, {} as never);
  expect(manifest).toEqual({ kind: "correction", id: expect.any(String), request });
});

it("derives the same manifest id across a retried attempt so the propagation is not re-run twice", async () => {
  const first = await maintenanceInput({} as never, assignment({ scheduled: { kind: "maintenance_correction", request }, attempt: 1 }), {} as never);
  const retried = await maintenanceInput({} as never, assignment({ scheduled: { kind: "maintenance_correction", request }, attempt: 1 }), {} as never);
  expect(retried).toEqual(first);
});

it("derives a correction manifest from a scheduled maintenance_source request the same way", async () => {
  const a = assignment({ scheduled: { kind: "maintenance_source", request } });
  const manifest = await maintenanceInput({} as never, a, {} as never);
  expect(manifest).toEqual({ kind: "correction", id: expect.any(String), request });
});

it("prefers a manifest artifact over a scheduled request when both are present", async () => {
  const a = assignment({ scheduled: { kind: "maintenance_correction", request }, artifacts: ["01a0bf4c-f7e5-7001-8000-000000000000"] });
  const published = { kind: "correction", id: "from-artifact", request };
  const host = { request: async () => ({ kind: "artifact_data", text: JSON.stringify(published) }) };
  const manifest = await maintenanceInput(host as never, a, {} as never);
  expect(manifest).toEqual(published);
});

it("fails clearly when neither a manifest artifact nor a scheduled request is present", async () => {
  await expect(maintenanceInput({} as never, assignment({}), {} as never)).rejects.toThrow(/manifest artifact.*or a scheduled request/);
});

it("an evaluation job the Host scheduled with no manifest reports a clear partial result naming the candidate, never a crash", async () => {
  const a = {
    job: {
      id: "01a0bf4c-f7e5-7000-8000-000000000000",
      attempt: 1,
      state: "running",
      spec: {
        brief: {
          scope: {},
          inputs: { memories: [{ memory_id: "01a0bf4c-f7e6-7694-b6b5-50f9604cdc8a", revision: 1, label: "Restart recovery method" }], sources: [], artifacts: [] },
        },
      },
    },
  } as unknown as Assignment;
  let completed: { action: string; result: { status: string; unresolved_work: string[] } } | undefined;
  const host = { request: vi.fn(async (req: unknown) => {
    completed = req as { action: string; result: { status: string; unresolved_work: string[] } };
    return { kind: "job", job: { state: "partial" } } as unknown as HostResponse;
  }) };
  const logs: string[] = [];
  const run = {
    host, lease: { assignment: a, fence: { job_id: a.job.id, owner_id: "o", epoch: 1 } }, session: {} as never,
    sessionsDirectory: ".", composition: "test", context, log: (l: string) => logs.push(l),
  } as unknown as ProcessRun;
  await evaluationHandler({}).run(run);
  expect(host.request).toHaveBeenCalledTimes(1);
  const complete = completed!;
  expect(complete.action).toBe("complete");
  expect(complete.result.status).toBe("partial");
  expect(complete.result.unresolved_work[0]).toContain("Restart recovery method");
  expect(logs.some((l) => l.includes("qualification unavailable"))).toBe(true);
});
