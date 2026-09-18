import { randomUUID } from "node:crypto";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { BACKGROUND_CONTEXT as context } from "@earendil-works/pi-agent-core";
import { NodeExecutionEnv } from "@earendil-works/pi-agent-core/node";
import {
  createAssistantMessageEventStream,
  createModels,
  fauxProvider,
  fauxAssistantMessage,
  fauxToolCall,
} from "@earendil-works/pi-ai";
import { expect, it, vi } from "vitest";
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type { HostRequest } from "../../../contracts/generated/host-request.js";
import type { HostResponse } from "../../../contracts/generated/host-response.js";
import { attachAssignedWorker } from "../src/assigned-worker.js";
import { openLocalSession } from "../src/local-session.js";
import { input, scriptedResult } from "./fixtures.js";

async function fixture() {
  const directory = await mkdtemp(join(tmpdir(), "memory-assigned-"));
  let local = await openLocalSession(directory, "assigned-test");
  const brief = input("before-correction").command.payload;
  brief.capabilities.tools = ["read"];
  const assignment: Assignment = {
    epoch: 1,
    owner_id: randomUUID(),
    expires_at: new Date(Date.now() + 30_000).toISOString(),
    job: {
      id: randomUUID(),
      root_id: randomUUID(),
      depth: 0,
      attempt: 1,
      session_id: local.session.metadata.id,
      operation_id: randomUUID(),
      state: "leased",
      deadline: new Date(Date.now() + 60_000).toISOString(),
      cancel_requested: false,
      spec: {
        brief,
        max_attempts: 3,
        retain_until: new Date(Date.now() + 120_000).toISOString(),
      },
    },
  };
  const calls: HostRequest[] = [];
  let failComplete: "before" | "after" | undefined;
  const host = {
    async request(command: HostRequest): Promise<HostResponse> {
      calls.push(command);
      if (command.action === "inspect_job" || command.action === "start")
        return { kind: "job", job: structuredClone(assignment.job) };
      if (command.action === "memory_changes")
        return {
          kind: "changes",
          page: {
            changes: [],
            cursor: command.after,
            snapshot_required: false,
          },
        };
      if (command.action === "renew" || command.action === "inspect_assignment")
        return { kind: "assignment", assignment };
      if (command.action === "reserve")
        return {
          kind: "reservation",
          reservation: {
            id: randomUUID(),
            job_id: assignment.job.id,
            budget_id: brief.limits.root_budget_id,
            provider_attempt: command.provider_attempt,
            maximum: command.maximum,
            final_result: false,
            knowledge: "reserved",
          },
        };
      if (command.action === "prepare_effect")
        return {
          kind: "effect",
          effect: {
            id: randomUUID(),
            job_id: assignment.job.id,
            epoch: assignment.epoch,
            request: command.request,
            state: "prepared",
          },
        };
      if (command.action === "complete") {
        if (failComplete === "before") {
          failComplete = undefined;
          throw new Error("Host unavailable before commit");
        }
        assignment.job.result = command.result;
        assignment.job.state =
          command.result.status === "complete" ? "completed" : "partial";
        if (failComplete === "after") {
          failComplete = undefined;
          throw new Error("Commit acknowledgement lost");
        }
        return { kind: "job", job: assignment.job };
      }
      return { kind: "done", affected: 1 };
    },
  };
  const faux = fauxProvider();
  const models = createModels();
  // Faux itself has no transport payload hook. This transport reaches the same
  // onPayload boundary as Pi's network providers, so the worker's final payload
  // check, provider control check and payload_checked manifest are exercised.
  models.setProvider({
    ...faux.provider,
    streamSimple(model, transcript, options) {
      const outer = createAssistantMessageEventStream();
      queueMicrotask(async () => {
        try {
          await options?.onPayload?.(transcript, model);
          const stream = faux.provider.streamSimple(model, transcript, options);
          for await (const event of stream) outer.push(event);
          outer.end(await stream.result());
        } catch (error) {
          const message = fauxAssistantMessage("", {
            stopReason: "error",
            errorMessage: String(error),
          });
          outer.push({ type: "error", reason: "error", error: message });
          outer.end(message);
        }
      });
      return outer;
    },
  });
  const profile = {
    ...brief.profile,
    maxPayloadBytes: 100_000,
    maxOutputTokens: faux.getModel().maxTokens,
    providerReservation: {
      tokens: 100_000 + faux.getModel().maxTokens,
      provider_calls: 1,
      cost_microunits: 0,
      sandbox_cpu_ms: 0,
      sandbox_time_ms: 0,
      output_bytes: 0,
    },
  };
  const options = {
    session: local.session,
    models,
    model: faux.getModel(),
    env: new NodeExecutionEnv({ cwd: directory }),
    tools: ["read" as const],
  };
  return {
    directory,
    local,
    assignment,
    calls,
    host,
    faux,
    options,
    profile,
    fail(where: "before" | "after") {
      failComplete = where;
    },
    async reopen() {
      await local.close();
      local = await openLocalSession(directory, "assigned-test");
      options.session = local.session;
    },
    attach: () => attachAssignedWorker(assignment, host, profile, options),
    async close() {
      await local.close();
      await rm(directory, { recursive: true, force: true });
    },
  };
}

it.each(["before", "after"] as const)(
  "reconciles a host failure %s commit without another provider call",
  async (failure) => {
    const f = await fixture();
    try {
      f.faux.setResponses([
        fauxAssistantMessage(
          JSON.stringify(scriptedResult("before-correction")),
        ),
      ]);
      f.fail(failure);
      const worker = await f.attach();
      await expect(worker.run()).rejects.toThrow(
        failure === "before" ? "before commit" : "acknowledgement lost",
      );
      expect(f.faux.state.callCount).toBe(1);
      const manifests=f.calls.filter(c=>c.action==="record_context");
      expect(manifests.some(c=>c.manifest.status==="payload_checked")).toBe(true);
      expect(manifests.some(c=>c.manifest.status==="responded")).toBe(true);
      expect(f.calls.some(c=>c.action==="check_provider")).toBe(true);
      const watch = await worker.watch();
      expect(await watch.resnapshot(context)).toBeDefined();
      watch.unsubscribe();
      await worker.close();
      await f.reopen();
      f.assignment.epoch++;
      const resumed = await f.attach();
      try {
        expect(await resumed.run()).toMatchObject({ kind: "settled" });
        expect(f.faux.state.callCount).toBe(1);
        const entries = await resumed.lane.findEntries(
          { type: "custom" },
          context,
        );
        expect(
          entries.filter(
            (e) => e.type === "custom" && e.customType === "memory.result",
          ),
        ).toHaveLength(1);
        const completes = f.calls.filter((c) => c.action === "complete");
        if (failure === "before")
          expect(completes[1].request_id).toBe(completes[0].request_id);
      } finally {
        await resumed.close();
      }
    } finally {
      await f.close();
    }
  },
);

it("reserves each provider step and records native tool receipts", async () => {
  const f = await fixture();
  try {
    await writeFile(join(f.directory, "evidence.txt"), "Source C");
    f.faux.setResponses([
      fauxAssistantMessage(fauxToolCall("read", { path: "evidence.txt" }), {
        stopReason: "toolUse",
      }),
      fauxAssistantMessage(JSON.stringify(scriptedResult("before-correction"))),
    ]);
    const worker = await f.attach();
    try {
      expect(await worker.run()).toMatchObject({ kind: "settled" });
      const reserves = f.calls.filter((c) => c.action === "reserve");
      expect(reserves).toHaveLength(2);
      expect(reserves[0].provider_attempt).not.toBe(
        reserves[1].provider_attempt,
      );
      expect(f.calls.filter((c) => c.action === "settle_usage")).toHaveLength(
        2,
      );
      expect(f.calls.find((c) => c.action === "prepare_effect")).toMatchObject({
        request: { kind: "read", replay: "observation" },
      });
      expect(f.calls.find((c) => c.action === "report_effect")).toMatchObject({
        state: "succeeded",
      });
    } finally {
      await worker.close();
    }
  } finally {
    await f.close();
  }
});

it("returns partial application work for malformed model output", async () => {
  const f = await fixture();
  try {
    f.faux.setResponses([fauxAssistantMessage("I have finished.")]);
    const worker = await f.attach();
    try {
      expect(await worker.run()).toMatchObject({
        kind: "settled",
        job: { state: "partial", result: { usage: { status: "unknown" } } },
      });
    } finally {
      await worker.close();
    }
  } finally {
    await f.close();
  }
});

it("persists a retry wait and resumes the same Pi operation", async () => {
  const f = await fixture();
  try {
    f.faux.setResponses([
      fauxAssistantMessage([], {
        stopReason: "error",
        errorMessage: "503 service unavailable",
      }),
      fauxAssistantMessage(JSON.stringify(scriptedResult("before-correction"))),
    ]);
    const worker = await f.attach();
    await worker.harness.setRetryPolicy(
      { enabled: true, maxRetries: 2, baseDelayMs: 150 },
      context,
    );
    expect(await worker.run()).toEqual({ kind: "waiting" });
    const wait = f.calls.find((c) => c.action === "wait");
    expect(wait).toMatchObject({ reason: "pi_retry" });
    await worker.close();
    await new Promise((resolve) => setTimeout(resolve, 160));
    await f.reopen();
    f.assignment.epoch++;
    const resumed = await f.attach();
    try {
      expect(await resumed.run()).toMatchObject({ kind: "settled" });
      expect(f.faux.state.callCount).toBe(2);
    } finally {
      await resumed.close();
    }
  } finally {
    await f.close();
  }
});

it("honours cancellation before calling the provider", async () => {
  const f = await fixture();
  try {
    f.assignment.job.cancel_requested = true;
    const lifecycle = {
      renew: vi.fn(async () => {}),
      finish: vi.fn(async () => {}),
    };
    const worker = await attachAssignedWorker(
      f.assignment,
      f.host,
      f.profile,
      f.options,
      context,
      lifecycle,
    );
    try {
      expect(await worker.run()).toEqual({ kind: "cancelled" });
      expect(f.faux.state.callCount).toBe(0);
      expect(lifecycle.finish).toHaveBeenCalledWith(true);
    } finally {
      await worker.close();
    }
  } finally {
    await f.close();
  }
});

it("resumes a deferred provider handle after the host wait", async () => {
  const f = await fixture();
  try {
    f.faux.setResponses([
      fauxAssistantMessage(JSON.stringify(scriptedResult("before-correction"))),
    ]);
    const worker = await f.attach();
    await worker.harness.setStreamOptions({ deferred: true }, context);
    expect(await worker.run()).toEqual({ kind: "waiting" });
    expect(f.calls.find((c) => c.action === "wait")).toMatchObject({
      reason: "pi_deferred",
    });
    await worker.close();
    await f.reopen();
    f.assignment.epoch++;
    const resumed = await f.attach();
    try {
      expect(await resumed.run()).toMatchObject({ kind: "settled" });
      expect(f.faux.state.callCount).toBe(1);
      expect(f.faux.state.deferredFetchCount).toBe(1);
    } finally {
      await resumed.close();
    }
  } finally {
    await f.close();
  }
});

it("retries sandbox export before completion without rerunning the model", async () => {
  const f = await fixture();
  const lifecycle = {
    renew: vi.fn(async () => {}),
    finish: vi
      .fn()
      .mockRejectedValueOnce(new Error("artifact unavailable"))
      .mockResolvedValue(undefined),
  };
  try {
    f.faux.setResponses([
      fauxAssistantMessage(JSON.stringify(scriptedResult("before-correction"))),
    ]);
    const worker = await attachAssignedWorker(
      f.assignment,
      f.host,
      f.profile,
      f.options,
      context,
      lifecycle,
    );
    try {
      await expect(worker.run()).rejects.toThrow("artifact unavailable");
      expect(f.calls.some((c) => c.action === "complete")).toBe(false);
      expect(await worker.run()).toMatchObject({ kind: "settled" });
      expect(f.faux.state.callCount).toBe(1);
      expect(lifecycle.finish).toHaveBeenCalledTimes(2);
    } finally {
      await worker.close();
    }
  } finally {
    await f.close();
  }
});

it("renews host ownership during export without reopening the fenced endpoint", async () => {
  const f = await fixture();
  const lifecycle = {
    renew: vi.fn(async () => {}),
    finish: vi.fn(async () => {
      await new Promise((resolve) => setTimeout(resolve, 5_100));
    }),
  };
  try {
    f.faux.setResponses([
      fauxAssistantMessage(JSON.stringify(scriptedResult("before-correction"))),
    ]);
    const worker = await attachAssignedWorker(
      f.assignment,
      f.host,
      f.profile,
      f.options,
      context,
      lifecycle,
    );
    try {
      expect(await worker.run()).toMatchObject({ kind: "settled" });
      expect(
        f.calls.filter((c) => c.action === "renew").length,
      ).toBeGreaterThanOrEqual(2);
      expect(lifecycle.renew).not.toHaveBeenCalled();
    } finally {
      await worker.close();
    }
  } finally {
    await f.close();
  }
}, 10_000);

it("does not call the provider when the host refuses its reservation", async () => {
  const f = await fixture();
  const request = f.host.request;
  f.host.request = async (command) => {
    if (command.action === "reserve") throw new Error("Root budget exhausted");
    return request(command);
  };
  try {
    f.faux.setResponses([
      fauxAssistantMessage(JSON.stringify(scriptedResult("before-correction"))),
    ]);
    const worker = await f.attach();
    try {
      const result = await worker.run();
      expect(result).toMatchObject({
        kind: "settled",
        job: { result: { status: "partial" } },
      });
      expect(f.faux.state.callCount).toBe(0);
    } finally {
      await worker.close();
    }
  } finally {
    await f.close();
  }
});

it("blocks a provider after revocation between reservation and final payload dispatch", async () => {
  const f = await fixture();
  const request = f.host.request.bind(f.host);
  let revoked = false;
  f.host.request = async (command) => {
    if (command.action === "inspect_assignment" && revoked) throw new Error("revoked");
    const result = await request(command);
    if (command.action === "reserve") revoked = true;
    return result;
  };
  try {
    const worker = await f.attach();
    try {
      // Pi may settle an error as a partial result; the egress invariant is no dispatch.
      await worker.run().catch(() => {});
      expect(revoked).toBe(true);
      expect(f.faux.state.callCount).toBe(0);
    } finally { await worker.close(); }
  } finally { await f.close(); }
});

it("denies restricted work when the profile has no approved provider retention", async () => {
  const f = await fixture();
  f.assignment.job.spec.brief.disclosure_policy = "restricted";
  try {
    const worker = await f.attach();
    try {
      await worker.run().catch(() => {});
      expect(f.faux.state.callCount).toBe(0);
      expect(f.calls.some(c => c.action === "reserve")).toBe(false);
    } finally { await worker.close(); }
  } finally { await f.close(); }
});
