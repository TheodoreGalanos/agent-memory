// ABOUTME: Evaluation process module for the pool: runs a scheduled qualification's held-out trials through
// ABOUTME: the trial-execution adapter, or reports a clear partial result when no manifest names a suite.
import type { WorkResult } from "../../../contracts/generated/assignment.js";
import { runQualification } from "../../consolidation/src/qualification.js";
import { trialExecutor } from "./adapters/trials.js";
import type { ProcessModule, Services } from "./composition.js";
import type { ProcessHandler, ProcessRun } from "./runner.js";
import { windowId } from "./formation.js";
import { inspectJson } from "../../host-client/src/artifact.js";
import { fenceFor } from "../../host-client/src/fence.js";
import type { HostCommands } from "../../host-client/src/client.js";
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type { Context } from "@earendil-works/pi-agent-core";

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

/** True only when the job's first input artifact is shaped like the `QualificationInput` manifest
 * `runQualification` needs (a `suite_id` naming a separate held-out suite artifact) — not merely present.
 * A job the Host's own scheduler created forwards whatever artifact the originating consolidation job
 * carried (often a bare suite, not a manifest combining it with the review and candidate), which
 * `runQualification` cannot use; distinguishing the two here, rather than only via a thrown error,
 * keeps the partial result's reason specific to what is actually missing. */
async function hasQualificationManifest(host: HostCommands, assignment: Assignment, context: Context): Promise<boolean> {
  const manifestId = assignment.job.spec.brief.inputs.artifacts[0];
  if (!manifestId) return false;
  try {
    const manifest = await inspectJson(host, fenceFor(assignment), manifestId, "", context);
    return typeof manifest === "object" && manifest !== null && typeof (manifest as { suite_id?: unknown }).suite_id === "string";
  } catch {
    return false;
  }
}

async function runEvaluationJob(run: ProcessRun, deps: EvaluationDeps) {
  const { host, lease, session, context, log } = run;
  const assignment = lease.assignment;
  const brief = assignment.job.spec.brief;
  const fence = lease.fence;
  if (assignment.job.state !== "running")
    await host.request({ action: "start", fence, composition: run.composition }, context.abortSignal);
  let result: WorkResult;
  if (!(await hasQualificationManifest(host, assignment, context))) {
    // A job the Host's own scheduler created directly from a committed candidate (no interactive
    // session attached a manifest naming its held-out suite): the candidate is named from
    // `inputs.memories[0]`, but held-out cases cannot be fabricated without one, so this reports a
    // clear partial result rather than attempting a comparison with no baseline to check it against.
    const label = brief.inputs.memories[0]?.label ?? "the candidate procedure";
    const reason = `No qualification manifest is available for ${label}; a held-out suite must be attached to the originating consolidation job before automatic qualification can run`;
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
  } else {
    const execute = trialExecutor(host, assignment, deps);
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
  }
  const committed = await host.request(
    { action: "complete", request_id: windowId(assignment.job.id, { source_id: "complete", revision: String(assignment.job.attempt) }), fence, result },
    context.abortSignal,
  );
  log(`job ${assignment.job.id} ${committed.kind === "job" ? committed.job.state : committed.kind}`);
}
