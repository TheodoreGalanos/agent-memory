// ABOUTME: Consolidation process module for the pool: derives a cohort selection from the assignment, drives
// ABOUTME: one bounded synthesis-and-review cycle through the task model, and publishes the combined result.
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type { ConsolidationResult } from "../../../contracts/generated/consolidation-result.js";
import type { CohortSelection } from "../../../contracts/generated/consolidation-review.js";
import type { HostResponse } from "../../../contracts/generated/host-response.js";
import type { WorkResult } from "../../../contracts/generated/work-result.js";
import { runConsolidation } from "../../consolidation/src/index.js";
import type { JudgementOptions } from "../../judgement/src/runtime.js";
import { catalogueResponses } from "../../judgement/src/responses.js";
import { synthesizeProposal } from "./adapters/synthesis.js";
import type { ProcessModule, Services } from "./composition.js";
import type { ProcessHandler, ProcessRun } from "./runner.js";
import { windowId } from "./formation.js";
import { observeProvider } from "./trace.js";

type ConsolidationDeps = Required<Pick<Services, "judgement">> & Pick<Services, "taskModel">;

/** Consolidation needs the judgement providers; synthesis needs the task model, but its absence still
 * settles the job (partial, naming the missing service) rather than blocking the whole pool. */
export const consolidationModule: ProcessModule = {
  declaration: { id: "consolidation", version: "1", requires: ["judgement"], optional: ["taskModel"], operations: ["consolidation"] },
  attach: (services) => [consolidationHandler({ judgement: services.judgement!, taskModel: services.taskModel })],
};

/** The persisted work a consolidation attempt needs, derived deterministically from its assignment so a
 * retried attempt resumes the same cohort instead of paying for a new one. Lane B's Host-timer scheduler
 * is expected to select cohort members; until then the brief's own inputs name them directly. */
export interface ConsolidationInput {
  id: string;
  selection: CohortSelection;
}

export function consolidationInput(assignment: Assignment): ConsolidationInput {
  const brief = assignment.job.spec.brief;
  if (brief.process !== "consolidation") throw new Error("Consolidation input needs a consolidation assignment");
  return {
    id: windowId(assignment.job.id, { source_id: "consolidation", revision: String(assignment.job.attempt) }),
    selection: {
      purpose: brief.purpose.trim().slice(0, 200) || "Consolidate related experience",
      mechanism: "scheduled",
      outcomes: brief.output_criteria,
      source_context: brief.definitions.join("; ").slice(0, 500) || brief.purpose,
      members: brief.inputs.memories,
    },
  };
}

export function consolidationHandler(deps: ConsolidationDeps): ProcessHandler<"consolidation"> {
  return { process: "consolidation", run: (run) => runConsolidationJob(run, deps) };
}

async function runConsolidationJob(run: ProcessRun, deps: ConsolidationDeps) {
  const { host, lease, session, context, log } = run;
  const assignment = lease.assignment;
  const brief = assignment.job.spec.brief;
  const fence = lease.fence;
  const input = consolidationInput(assignment);
  if (assignment.job.state !== "running")
    await host.request({ action: "start", fence, composition: run.composition }, context.abortSignal);
  const { primary, fallback, comparison } = deps.judgement;
  const judgement: JudgementOptions = {
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
  const taskModel = deps.taskModel;
  let coverage = { examined: [] as string[], unexamined: [] as string[] };
  let unresolved: string[] = [];
  let childOutputs: string[] = [];
  if (!taskModel) {
    unresolved = ["Consolidation needs the task model service to synthesize a proposal; none is installed"];
    log(`job ${assignment.job.id} consolidation unavailable: no task model installed`);
  } else {
    try {
      const result: ConsolidationResult = await runConsolidation(
        host, assignment, session, input,
        { ...judgement, synthesize: (window, ctx) => synthesizeProposal(taskModel, window, ctx) },
        context,
      );
      coverage = result.coverage;
      unresolved = result.deferred;
      const published = await host.request(
        { action: "publish_artifact", fence, request_id: windowId(assignment.job.id, { source_id: "consolidation-result", revision: String(assignment.job.attempt) }), label: "Consolidation result", text: JSON.stringify(result), dependencies: [] },
        context.abortSignal,
      );
      if (published.kind !== "artifact") throw new Error("Consolidation result was not published");
      childOutputs = [published.artifact.id];
      log(`job ${assignment.job.id} consolidation: ${result.record ? "candidate retained" : "deferred"} (${result.independent_sources} independent source(s))`);
    } catch (error) {
      const reason = error instanceof Error ? error.message : String(error);
      unresolved = [reason];
      log(`job ${assignment.job.id} consolidation unavailable: ${reason}`);
    }
  }
  const completion: WorkResult = {
    schema_version: "1",
    status: unresolved.length ? "partial" : "complete",
    examined_scope: brief.scope,
    inputs: brief.inputs,
    findings: [],
    coverage,
    unresolved_work: unresolved,
    proposed_changes: [],
    child_outputs: childOutputs,
    known_effects: [],
    usage: { status: "unknown", input_tokens: null, output_tokens: null, cost: null },
  };
  const committed: HostResponse = await host.request(
    { action: "complete", request_id: windowId(assignment.job.id, { source_id: "complete", revision: String(assignment.job.attempt) }), fence, result: completion },
    context.abortSignal,
  );
  log(`job ${assignment.job.id} ${committed.kind === "job" ? committed.job.state : committed.kind}`);
}
