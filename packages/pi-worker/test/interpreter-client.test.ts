import { randomUUID } from "node:crypto";
import { BACKGROUND_CONTEXT as context } from "@earendil-works/pi-agent-core";
import { expect, it, vi } from "vitest";
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type { HostRequest } from "../../../contracts/generated/host-request.js";
import type {
  Effect,
  HostResponse,
} from "../../../contracts/generated/host-response.js";
import {
  InterpreterClient,
  type InterpreterReply,
} from "../src/interpreter-client.js";
import { input } from "./fixtures.js";

function fixture() {
  const assignment: Assignment = {
    owner_id: randomUUID(),
    epoch: 1,
    expires_at: new Date(Date.now() + 30_000).toISOString(),
    job: {
      id: randomUUID(),
      root_id: randomUUID(),
      operation_id: randomUUID(),
      session_id: randomUUID(),
      state: "running",
      depth: 0,
      attempt: 1,
      cancel_requested: false,
      deadline: new Date(Date.now() + 60_000).toISOString(),
      spec: {
        brief: input("before-correction").command.payload,
        max_attempts: 3,
        retain_until: new Date(Date.now() + 60_000).toISOString(),
      },
    },
  };
  assignment.job.spec.brief.capabilities.tools.push("python");
  let effect: Effect | undefined;
  const calls: HostRequest[] = [];
  const host = {
    request: vi.fn(async (command: HostRequest): Promise<HostResponse> => {
      calls.push(command);
      switch (command.action) {
        case "inspect_assignment":
          return { kind: "assignment", assignment };
        case "prepare_effect":
          effect ??= {
            id: randomUUID(),
            job_id: assignment.job.id,
            epoch: 1,
            state: "prepared",
            request: command.request,
          };
          return { kind: "effect", effect };
        case "begin_effect":
          effect!.state = "in_progress";
          return { kind: "effect", effect: effect! };
        case "report_effect":
          effect!.state = command.state;
          effect!.receipt = command.receipt;
          return { kind: "effect", effect: effect! };
        case "reserve":
          return {
            kind: "reservation",
            reservation: {
              id: randomUUID(),
              job_id: assignment.job.id,
              budget_id: assignment.job.spec.brief.limits.root_budget_id,
              maximum: command.maximum,
              provider_attempt: command.provider_attempt,
              final_result: false,
              knowledge: "reserved",
            },
          };
        case "settle_usage":
          return { kind: "done", affected: 1 };
        default:
          throw new Error(`Unexpected ${command.action}`);
      }
    }),
  };
  const transport = {
    interpreter: vi.fn(async (): Promise<InterpreterReply> => ({
      status: "ok",
      generation: "fixture",
      output: "7\n",
    })),
  };
  const client = new InterpreterClient(
    transport,
    host,
    assignment,
    randomUUID(),
  );
  return { client, host, transport, assignment, calls, effect: () => effect };
}

it("reserves interpreter resources and reuses completed execution receipts", async () => {
  const f = fixture();
  await f.client.start(context);
  const id = randomUUID();
  expect((await f.client.execute(id, "print(7)", context)).output).toBe("7\n");
  expect((await f.client.execute(id, "print(7)", context)).output).toBe("7\n");
  expect(f.transport.interpreter).toHaveBeenCalledTimes(2);
  expect(f.calls).toContainEqual(
    expect.objectContaining({
      action: "reserve",
      maximum: expect.objectContaining({
        sandbox_time_ms: 0,
        output_bytes: 8192,
      }),
    }),
  );
  expect(f.calls).toContainEqual(
    expect.objectContaining({
      action: "settle_usage",
      observed: expect.objectContaining({ output_bytes: expect.any(Number) }),
    }),
  );
});

it("does not dispatch code when the root budget refuses a reservation", async () => {
  const f = fixture();
  await f.client.start(context);
  const request = f.host.request.getMockImplementation()!;
  f.host.request.mockImplementation(async (command) => {
    if (command.action === "reserve") throw new Error("Root budget exhausted");
    return request(command);
  });
  await expect(
    f.client.execute(randomUUID(), "print(7)", context),
  ).rejects.toThrow("budget exhausted");
  expect(f.transport.interpreter).toHaveBeenCalledTimes(1);
  expect(f.effect()?.state).toBe("prepared");
});

it("a lost execution reply requires reconciliation and cannot replay", async () => {
  const f = fixture();
  await f.client.start(context);
  f.transport.interpreter.mockRejectedValueOnce(new Error("SSH disconnected"));
  const id = randomUUID();
  await expect(f.client.execute(id, "change_files()", context)).rejects.toThrow(
    "SSH disconnected",
  );
  expect(f.effect()?.state).toBe("outcome_unknown");
  await expect(f.client.execute(id, "change_files()", context)).rejects.toThrow(
    "needs reconciliation",
  );
  expect(f.transport.interpreter).toHaveBeenCalledTimes(2);
});

it("cancellation prevents starting or using the interpreter", async () => {
  const f = fixture();
  await f.client.start(context);
  f.assignment.job.cancel_requested = true;
  await expect(f.client.start(context)).rejects.toThrow("inactive");
  await expect(
    f.client.checkpoint(["items"], randomUUID(), context),
  ).rejects.toThrow("inactive");
  expect(f.transport.interpreter).toHaveBeenCalledTimes(1);
});

it("can stop a lost heap discovered by a newly attached client", async () => {
  const f = fixture();
  f.transport.interpreter.mockResolvedValueOnce({
    status: "lost",
    generation: "previous-heap",
  });
  await expect(f.client.start(context)).rejects.toThrow("heap was lost");
  await f.client.stop(context);
  expect(f.transport.interpreter).toHaveBeenLastCalledWith(
    expect.objectContaining({ action: "stop", generation: "previous-heap" }),
    context,
  );
});
