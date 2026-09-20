// ABOUTME: Investigation process module for the pool: retrieves relevant memories for the task at admission,
// ABOUTME: attaches the assigned Pi worker with the task model and granted tools, and drives Pi to a settled result.
import { NodeExecutionEnv } from "@earendil-works/pi-agent-core/node";
import type { ExecutionEnv } from "@earendil-works/pi-agent-core";
import type { Model, Api, Models } from "@earendil-works/pi-ai";
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type { ActivationQuery, ContextPackage } from "../../../contracts/generated/host-response.js";
import { activationAccess, applyActivation, runActivation } from "../../activation/src/index.js";
import type { JudgementOptions } from "../../judgement/src/runtime.js";
import { catalogueResponses } from "../../judgement/src/responses.js";
import { AspExecutionEnv } from "../../pi-worker/src/asp-execution-env.js";
import { attachAssignedWorker, type WorkerProfile } from "../../pi-worker/src/assigned-worker.js";
import { EXECUTION_TOOLS, toolName, type ExecutionTool, type WorkerTool } from "../../pi-worker/src/harness.js";
import { InterpreterClient } from "../../pi-worker/src/interpreter-client.js";
import { workspaceFromBrief } from "../../pi-worker/src/workspace.js";
import type { HostCommands } from "../../host-client/src/client.js";
import { selectAdapter } from "./adapter.js";
import type { ProcessModule, Services } from "./composition.js";
import type { ProcessHandler, ProcessRun } from "./runner.js";
import { windowId } from "./formation.js";
import { constrainToWorkResult, deliverInputMemories, taskSystemPrompt } from "./task.js";
import { observeProvider } from "./trace.js";
import { checkResultCoverage, reactiveDecisionHint } from "./decision.js";

export interface InvestigationDeps {
  models: Models;
  taskModel: Model<Api>;
  maxPayloadBytes: number;
  /** Cost reservation per task provider call, in microunits. Settles to observed usage. */
  taskReservationCost?: number;
  /** Application tools other modules offer; only those the brief grants reach the model. */
  tools?: WorkerTool[];
  /** When installed, relevant memories are retrieved for the task before the model runs. */
  judgement?: Services["judgement"];
  /** Where the task's granted native tools run. Absent means the task is tool-less. */
  execution?: Services["execution"];
  /** Installed task adapters a brief's profile label may select; absent uses only the default. */
  adapters?: Services["adapters"];
}

/** Investigation drives the task model; offered tools are attached when the brief grants them. */
export const investigationModule: ProcessModule = {
  declaration: { id: "investigation", version: "3", requires: ["taskModel"], optional: ["judgement", "execution", "adapters"], operations: ["investigation"] },
  attach: (services: Services, offered) => {
    const task = services.taskModel!;
    return [investigationHandler({
      models: task.models, taskModel: task.model, maxPayloadBytes: task.maxPayloadBytes,
      taskReservationCost: task.reservationCost, tools: offered, judgement: services.judgement, execution: services.execution,
      adapters: services.adapters,
    })];
  },
};

/** The retrieval a new task asks for: its purpose as the question, its brief as task evidence, within its
 * own scope and payload allowance. Memories the brief already grants count as existing context. */
export function admissionQuery(assignment: Assignment, maxPayloadBytes: number): ActivationQuery {
  const brief = assignment.job.spec.brief;
  return {
    scope: brief.scope,
    question: brief.purpose.slice(0, 2048),
    task_context: JSON.stringify({ purpose: brief.purpose, output_criteria: brief.output_criteria, inputs: brief.inputs }).slice(0, 8192),
    entities: brief.scope.entity_ids.slice(0, 32),
    exact: [],
    families: [],
    valid_at: null,
    recorded_as_of: null,
    fresh_after: null,
    vector: null,
    existing: brief.inputs.memories.slice(0, 100),
    scan_limit: 200,
    candidate_limit: 16,
    traversal_limit: 16,
    context_bytes: Math.min(60_000, Math.floor(maxPayloadBytes / 2)),
  };
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

/** What the worker runs Pi's native tools under, and which of the brief's granted native tool
 * names the harness can actually honor with it. Absent `execution`: tool-less, a plain node
 * environment rooted in the pool's sessions directory (no native tool names are granted). A node
 * environment has no sandbox interpreter, so `python` is never granted under it even if the brief
 * lists it. An `asp` environment supplies its own interpreter transport, so `python` is granted
 * whenever the brief lists it. */
async function resolveExecution(
  execution: Services["execution"],
  deps: { host: HostCommands; assignment: Assignment; sessionsDirectory: string; log: (line: string) => void },
): Promise<{ env: ExecutionEnv; interpreter?: InterpreterClient; native: ExecutionTool[] }> {
  const brief = deps.assignment.job.spec.brief;
  const granted = (name: ExecutionTool) => brief.capabilities.tools.includes(name);
  if (!execution) return { env: new NodeExecutionEnv({ cwd: deps.sessionsDirectory }), native: [] };
  if (execution.kind === "node") {
    if (granted("python")) deps.log(`job ${deps.assignment.job.id} brief grants python but the execution service is a node environment with no interpreter; not granted`);
    return {
      env: new NodeExecutionEnv({ cwd: execution.cwd ?? deps.sessionsDirectory }),
      native: EXECUTION_TOOLS.filter((name) => name !== "python" && granted(name)),
    };
  }
  const env = await AspExecutionEnv.connect(execution.binding, execution.binding, execution.helperPath);
  const interpreter = granted("python") ? new InterpreterClient(env, deps.host, deps.assignment, deps.assignment.job.id) : undefined;
  return { env, interpreter, native: EXECUTION_TOOLS.filter((name) => granted(name)) };
}

async function runInvestigation(run: ProcessRun, deps: InvestigationDeps) {
  const { host, lease, session, context, log } = run;
  const assignment = lease.assignment;
  const brief = assignment.job.spec.brief;
  const fence = lease.fence;
  const model = deps.taskModel;
  // Retrieve what the project already knows before the model runs. Nothing retrieved, or context
  // judged sufficient, is a valid outcome; an unavailable retrieval is reported and the task proceeds.
  let activated: ContextPackage | undefined;
  let judgement: JudgementOptions | undefined;
  if (deps.judgement) {
    const { primary, fallback, comparison } = deps.judgement;
    judgement = {
      primary: observeProvider(primary, "primary", log),
      fallback: fallback ? observeProvider(fallback, "fallback", log) : undefined,
      comparison: comparison ? { provider: observeProvider(comparison.provider, "comparison", log) } : undefined,
      maxAttempts: 1,
      responses: catalogueResponses,
      disclosure: {
        policy: brief.disclosure_policy,
        scope: brief.scope,
        providers: [primary.id, ...(fallback ? [fallback.id] : []), ...(comparison ? [comparison.provider.id] : [])],
      },
    };
    try {
      activated = await runActivation(
        host, assignment, session,
        { id: windowId(assignment.job.id, { source_id: "activation", revision: String(assignment.job.attempt) }), query: admissionQuery(assignment, deps.maxPayloadBytes) },
        judgement, context,
      );
      log(`job ${assignment.job.id} activation: ${activated.selected.length} memory(ies) selected, ${activated.deferred.length} deferred`);
    } catch (error) {
      log(`job ${assignment.job.id} activation unavailable: ${error instanceof Error ? error.message : String(error)}; running without retrieved memories`);
    }
  }
  const initial = workspaceFromBrief(brief, assignment.job.deadline, assignment.job.spec.retain_until);
  const { env, interpreter, native } = await resolveExecution(deps.execution, {
    host, assignment, sessionsDirectory: run.sessionsDirectory, log,
  });
  const adapter = selectAdapter(brief, deps.adapters);
  const decides = brief.capabilities.tools.includes("decide") && (deps.tools ?? []).some((tool) => toolName(tool) === "decide");
  // The worker follows the runner's lease keeper and owns Pi's cancellation protocol (abort the
  // operation, then acknowledge), so it runs under the lease context rather than the cancel-aborting run context.
  const worker = await attachAssignedWorker(assignment, host, investigationProfile(assignment, deps), {
    session,
    models: deps.models,
    model,
    env,
    interpreter,
    tools: [...native, ...(deps.tools ?? []).filter((tool) => brief.capabilities.tools.includes(toolName(tool)))],
    systemPrompt: taskSystemPrompt(brief),
    lease,
    composition: run.composition,
    // An independent adapter validates the result before it commits; the model's own claims about
    // its status or coverage are never taken as proof of meeting the brief's output criteria.
    beforeComplete: async (result, experience, hookContext) => {
      // With the decision attachment granted, a coverage check compares the result's claims against
      // what actually happened; a concern is appended as unresolved work rather than blocking.
      let checked = result;
      if (decides && judgement) {
        try {
          checked = await checkResultCoverage(host, assignment, session, result, experience, judgement, hookContext);
        } catch (error) {
          log(`job ${assignment.job.id} coverage check unavailable: ${error instanceof Error ? error.message : String(error)}`);
        }
      }
      const { verdict, reasons } = adapter.validate(brief, checked, experience);
      const recorded = await host.request({ action: "record_validation", fence, verdict, reasons }, hookContext.abortSignal);
      if (recorded.kind !== "job") throw new Error("Host did not record the validation verdict");
      return checked === result ? undefined : checked;
    },
    ...(activated
      ? {
          workspace: { initial: applyActivation(initial, activated) },
          grant: (base) => activationAccess(base, activated!),
        }
      : {}),
  }, lease.context);
  try {
    // Hook failures fail the provider call; name them so an operator sees why a task went partial.
    worker.harness.events.on("handler_error", (event) => {
      log(`job ${assignment.job.id} harness ${event.kind} error: ${String(event.error)}`);
    });
    if (model.api === "azure-openai-responses")
      worker.harness.hooks.on("before_payload", async ({ payload }) => ({ payload: constrainToWorkResult(payload) }));
    // Repeated failing or identical tool calls prompt the model to ask the decision attachment.
    if (decides) worker.harness.hooks.on("transform_context", reactiveDecisionHint());
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
        await host.request({ action: "start", fence, composition: run.composition }, context.abortSignal);
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
