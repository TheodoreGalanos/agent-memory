// ABOUTME: Investigation process handler for the pool: attaches the assigned Pi worker with the task model,
// ABOUTME: delivers granted input memories, and drives Pi's operation state machine to a settled result.
import { NodeExecutionEnv } from "@earendil-works/pi-agent-core/node";
import type { Model, Api, Models } from "@earendil-works/pi-ai";
import type { Assignment } from "../../../contracts/generated/assignment.js";
import { attachAssignedWorker, type WorkerProfile } from "../../pi-worker/src/assigned-worker.js";
import type { ProcessHandler, ProcessRun } from "./runner.js";
import { windowId } from "./formation.js";
import { constrainToWorkResult, deliverInputMemories, taskSystemPrompt } from "./task.js";

export interface InvestigationDeps {
  models: Models;
  taskModel: Model<Api>;
  maxPayloadBytes: number;
  /** Cost reservation per task provider call, in microunits. Settles to observed usage. */
  taskReservationCost?: number;
}

/** The worker profile an investigation attempt runs under, derived from the brief and the pool's limits. */
export function investigationProfile(assignment: Assignment, deps: InvestigationDeps): WorkerProfile {
  const brief = assignment.job.spec.brief;
  if (brief.process !== "investigation") throw new Error("Investigation profile needs an investigation assignment");
  return {
    ...brief.profile,
    maxPayloadBytes: deps.maxPayloadBytes,
    maxOutputTokens: deps.taskModel.maxTokens,
    providerReservation: {
      tokens: deps.maxPayloadBytes + deps.taskModel.maxTokens,
      provider_calls: 1,
      cost_microunits: deps.taskReservationCost ?? 100_000,
      output_bytes: 0,
      sandbox_cpu_ms: 0,
      sandbox_time_ms: 0,
    },
  };
}

export function investigationHandler(deps: InvestigationDeps): ProcessHandler<"investigation"> {
  return { process: "investigation", run: (run) => runInvestigation(run, deps) };
}

async function runInvestigation(run: ProcessRun, deps: InvestigationDeps) {
  const { host, lease, session, context, log } = run;
  const assignment = lease.assignment;
  const brief = assignment.job.spec.brief;
  const fence = lease.fence;
  const model = deps.taskModel;
  // The worker follows the runner's lease keeper and owns Pi's cancellation protocol (abort the
  // operation, then acknowledge), so it runs under the lease context rather than the cancel-aborting run context.
  const worker = await attachAssignedWorker(assignment, host, investigationProfile(assignment, deps), {
    session,
    models: deps.models,
    model,
    env: new NodeExecutionEnv({ cwd: run.sessionsDirectory }),
    tools: [],
    systemPrompt: taskSystemPrompt(brief),
    lease,
  }, lease.context);
  try {
    if (model.api === "azure-openai-responses")
      worker.harness.hooks.on("before_payload", async ({ payload }) => ({ payload: constrainToWorkResult(payload) }));
    if (brief.inputs.memories.length) {
      const delivered = await host.request(
        { action: "current_memories", fence, references: brief.inputs.memories },
        context.abortSignal,
      );
      if (delivered.kind !== "memories") throw new Error("Host did not return current memories");
      try {
        await worker.workspace.checkpoint(deliverInputMemories(await worker.workspace.state(), delivered.memories));
      } catch (error) {
        // A granted input that was revised or withdrawn blocks this brief; it is not a transient failure.
        const reason = error instanceof Error ? error.message : String(error);
        await host.request({ action: "start", fence }, context.abortSignal);
        await host.request({
          action: "complete", request_id: windowId(assignment.job.id, { source_id: "blocked", revision: String(assignment.job.attempt) }), fence,
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
}
