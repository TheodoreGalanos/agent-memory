import type { Context } from "@earendil-works/pi-agent-core";
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type { HostCommands } from "./host-client.js";
import { inspectJson } from "./scoped-operation.js";

export interface InterpreterReply {
  status: "ok" | "failed" | "unknown" | "lost";
  generation?: string;
  output?: string;
  objects?: Record<string, unknown>;
  error?: string;
}
export interface InterpreterTransport {
  interpreter(
    request: Record<string, unknown>,
    context: Context,
  ): Promise<InterpreterReply>;
}

/** Trusted worker-mediated calls. Generated Python never receives this host client. */
export class InterpreterClient {
  private generation?: string;
  private readonly fence;
  constructor(
    private readonly transport: InterpreterTransport,
    private readonly host: HostCommands,
    private readonly assignment: Assignment,
    readonly sessionId: string,
  ) {
    if (!assignment.job.spec.brief.capabilities.tools.includes("python"))
      throw new Error("Python is outside the assignment capability set");
    this.fence = {
      job_id: assignment.job.id,
      owner_id: assignment.owner_id,
      epoch: assignment.epoch,
    };
  }
  async start(context: Context) {
    const checked = await this.host.request(
      { action: "inspect_assignment", fence: this.fence },
      context.abortSignal,
    );
    if (
      checked.kind !== "assignment" ||
      checked.assignment.job.cancel_requested
    )
      throw new Error("Interpreter assignment is inactive");
    const remaining = Math.floor(
      (Date.parse(this.assignment.job.deadline) - Date.now()) / 1000,
    );
    if (remaining < 1) throw new Error("Interpreter job deadline has passed");
    const reply = await this.transport.interpreter(
      {
        action: "start",
        session_id: this.sessionId,
        lifetime_seconds: Math.min(300, remaining),
        idle_seconds: Math.min(30, remaining),
      },
      context,
    );
    this.generation = reply.generation;
    if (reply.status !== "ok" || !reply.generation)
      throw new Error(
        "Interpreter heap was lost; stop, restart and restore a checkpoint explicitly",
      );
    return reply;
  }
  private async call(request: Record<string, unknown>, context: Context) {
    if (!this.generation) throw new Error("Start the scoped interpreter first");
    const check = await this.host.request(
      { action: "inspect_assignment", fence: this.fence },
      context.abortSignal,
    );
    if (check.kind !== "assignment" || check.assignment.job.cancel_requested)
      throw new Error("Interpreter assignment is inactive");
    return this.transport.interpreter(
      { ...request, session_id: this.sessionId, generation: this.generation },
      context,
    );
  }
  async execute(
    requestId: string,
    code: string,
    context: Context,
  ): Promise<InterpreterReply> {
    if (Buffer.byteLength(code) > 32768)
      throw new Error("Interpreter code exceeds 32 KiB");
    const prepared = await this.host.request(
      {
        action: "prepare_effect",
        fence: this.fence,
        request: {
          logical_operation_id: `${this.assignment.job.operation_id}/${requestId}`,
          invocation_id: requestId,
          kind: "python",
          replay: "reconcile",
          arguments: { session_id: this.sessionId, code },
        },
      },
      context.abortSignal,
    );
    if (prepared.kind !== "effect")
      throw new Error("Host did not prepare interpreter execution");
    if (
      prepared.effect.state === "succeeded" ||
      prepared.effect.state === "failed"
    )
      return prepared.effect.receipt as unknown as InterpreterReply;
    // A prior in-flight operation requires reconciliation even if a new interpreter exists.
    if (prepared.effect.state !== "prepared")
      throw new Error(
        "Interpreter execution needs reconciliation; it will not be replayed",
      );
    // The sandbox allocation already reserves its CPU and lifetime ceiling.
    // Reserve returned output here without charging the same compute twice.
    const maximum = {
      tokens: 0,
      cost_microunits: 0,
      provider_calls: 0,
      sandbox_cpu_ms: 0,
      sandbox_time_ms: 0,
      output_bytes: 8192,
    };
    const reservation = await this.host.request(
      {
        action: "reserve",
        fence: this.fence,
        provider_attempt: `interpreter:${requestId}`,
        maximum,
        final_result: false,
      },
      context.abortSignal,
    );
    if (reservation.kind !== "reservation")
      throw new Error("Host did not reserve interpreter resources");
    await this.host.request(
      {
        action: "begin_effect",
        fence: this.fence,
        effect_id: prepared.effect.id,
      },
      context.abortSignal,
    );
    let reply: InterpreterReply;
    try {
      reply = await this.call(
        { action: "execute", request_id: requestId, code, timeout_seconds: 3 },
        context,
      );
    } catch (error) {
      await this.host.request({
        action: "report_effect",
        fence: this.fence,
        effect_id: prepared.effect.id,
        state: "outcome_unknown",
        receipt: { reason: "Interpreter reply unavailable" },
      });
      await this.host.request({
        action: "settle_usage",
        fence: this.fence,
        reservation_id: reservation.reservation.id,
        observed: null,
      });
      throw error;
    }
    await this.host.request(
      {
        action: "report_effect",
        fence: this.fence,
        effect_id: prepared.effect.id,
        state:
          reply.status === "ok"
            ? "succeeded"
            : reply.status === "failed"
              ? "failed"
              : "outcome_unknown",
        receipt: JSON.parse(JSON.stringify(reply)),
      },
      context.abortSignal,
    );
    // The worker observes returned bytes; sandbox CPU billing remains with the allocation.
    await this.host.request(
      {
        action: "settle_usage",
        fence: this.fence,
        reservation_id: reservation.reservation.id,
        observed: {
          ...maximum,
          output_bytes: Buffer.byteLength(JSON.stringify(reply)),
        },
      },
      context.abortSignal,
    );
    return reply;
  }
  async checkpoint(names: string[], artifactId: string, context: Context) {
    const reply = await this.call({ action: "checkpoint", names }, context);
    if (reply.status !== "ok" || !reply.objects)
      throw new Error("Interpreter checkpoint failed");
    const response = await this.host.request(
      {
        action: "publish_artifact",
        fence: this.fence,
        request_id: artifactId,
        label: "Interpreter application checkpoint",
        text: JSON.stringify({ objects: reply.objects }),
        dependencies: this.assignment.job.spec.brief.inputs.artifacts,
      },
      context.abortSignal,
    );
    if (response.kind !== "artifact")
      throw new Error("Checkpoint publication failed");
    return response.artifact.id;
  }
  async restore(artifactId: string, context: Context) {
    const objects = await inspectJson(
      this.host,
      this.fence,
      artifactId,
      "/objects",
      context,
    );
    return this.call({ action: "restore", objects }, context);
  }
  async receipt(requestId: string, context: Context) {
    return this.call({ action: "receipt", request_id: requestId }, context);
  }
  async stop(context: Context) {
    if (this.generation) return this.call({ action: "stop" }, context);
  }
}
