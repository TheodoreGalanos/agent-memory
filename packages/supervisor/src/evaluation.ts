// ABOUTME: Evaluation process module for the pool: runs a scheduled qualification's held-out trials through
// ABOUTME: the trial-execution adapter and publishes the comparison report `runQualification` already builds.
import type { WorkResult } from "../../../contracts/generated/assignment.js";
import { runQualification } from "../../consolidation/src/qualification.js";
import { trialExecutor } from "./adapters/trials.js";
import type { ProcessModule, Services } from "./composition.js";
import type { ProcessHandler, ProcessRun } from "./runner.js";
import { windowId } from "./formation.js";

type EvaluationDeps = Pick<Services, "execution" | "taskModel">;

/** Evaluation (qualification) needs no judgement of its own; its trials run through the task model or the
 * assigned sandbox execution service, whichever a candidate's arm needs. Both are optional per trial: a
 * missing service fails only the trials that need it, reported as this job's unresolved work. */
export const evaluationModule: ProcessModule = {
  declaration: { id: "evaluation", version: "1", requires: [], optional: ["execution", "taskModel"], operations: ["evaluation"] },
  attach: (services) => [evaluationHandler({ execution: services.execution, taskModel: services.taskModel })],
};

export function evaluationHandler(deps: EvaluationDeps): ProcessHandler<"evaluation"> {
  return { process: "evaluation", run: (run) => runEvaluationJob(run, deps) };
}

async function runEvaluationJob(run: ProcessRun, deps: EvaluationDeps) {
  const { host, lease, session, context, log } = run;
  const assignment = lease.assignment;
  const brief = assignment.job.spec.brief;
  const fence = lease.fence;
  if (assignment.job.state !== "running")
    await host.request({ action: "start", fence, composition: run.composition }, context.abortSignal);
  const execute = trialExecutor(host, assignment, deps);
  let result: WorkResult;
  try {
    result = await runQualification(host, assignment, session, execute, context);
    log(`job ${assignment.job.id} qualification: ${result.coverage.examined.length} case(s) compared`);
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    log(`job ${assignment.job.id} qualification unavailable: ${reason}`);
    result = {
      schema_version: "1",
      status: "partial",
      examined_scope: brief.scope,
      inputs: brief.inputs,
      findings: [],
      coverage: { examined: [], unexamined: [] },
      unresolved_work: [reason],
      proposed_changes: [],
      child_outputs: [],
      known_effects: [],
      usage: { status: "unknown", input_tokens: null, output_tokens: null, cost: null },
    };
  }
  const committed = await host.request(
    { action: "complete", request_id: windowId(assignment.job.id, { source_id: "complete", revision: String(assignment.job.attempt) }), fence, result },
    context.abortSignal,
  );
  log(`job ${assignment.job.id} ${committed.kind === "job" ? committed.job.state : committed.kind}`);
}
