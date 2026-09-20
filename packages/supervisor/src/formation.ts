import { requestId } from "../../host-client/src/identity.js";
// ABOUTME: Formation process module for the pool: derives one bounded capture window per input source
// ABOUTME: from the assignment, runs each under the runner's lease, and publishes the combined WorkResult.
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type { FormationResult } from "../../../contracts/generated/formation-result.js";
import type { FormationWindow } from "../../../contracts/generated/formation-window.js";
import type { HostResponse } from "../../../contracts/generated/host-response.js";
import type { WorkResult } from "../../../contracts/generated/work-result.js";
import { runFormation } from "../../formation/src/index.js";
import type { ProcessModule, Services } from "./composition.js";
import type { ProcessHandler, ProcessRun } from "./runner.js";
import { judgementFor } from "./judgement.js";

type FormationDeps = Required<Pick<Services, "judgement">>;

/** Formation needs the judgement providers; it drives no task model. */
export const formationModule: ProcessModule = {
  declaration: { id: "formation", version: "1", requires: ["judgement"], operations: ["formation"] },
  attach: (services) => [formationHandler({ judgement: services.judgement! })],
};

/** The persisted work a formation attempt needs, derived deterministically from its assignment so a
 * retried attempt resumes the same windows instead of paying again. */
export interface FormationInput {
  /** The brief's purpose names the operation: the same purpose continues a source's cursor, a new
   * purpose explicitly reinterprets the source. */
  operation: string;
  windows: { id: string; source: FormationWindow["source"]; limit: number }[];
}

/** Windows one formation job works through per source before leaving the rest to a later job. */
const MAX_WINDOWS_PER_SOURCE = 8;

export function formationInput(assignment: Assignment): FormationInput {
  const brief = assignment.job.spec.brief;
  if (brief.process !== "formation") throw new Error("Formation input needs a formation assignment");
  return {
    operation: brief.purpose.trim().slice(0, 200) || "capture",
    windows: brief.inputs.sources.map((source) => ({ id: requestId(assignment.job.id, source.source_id, source.revision), source, limit: 32 })),
  };
}

export function formationHandler(deps: FormationDeps): ProcessHandler<"formation"> {
  return { process: "formation", run: (run) => runFormationJob(run, deps) };
}

async function runFormationJob(run: ProcessRun, deps: FormationDeps) {
  const { host, lease, session, context, log } = run;
  const assignment = lease.assignment;
  const brief = assignment.job.spec.brief;
  const fence = lease.fence;
  const input = formationInput(assignment);
  if (assignment.job.state !== "running")
    await host.request({ action: "start", fence, composition: run.composition }, context.abortSignal);
  const judgement = judgementFor(deps.judgement, brief, log);
  const results: FormationResult[] = [];
  for (const window of input.windows) {
    // Continue bounded windows while the source has more, up to the per-source allowance; what is
    // left is reported as unexamined and a later job continues the cursor.
    for (let continuation = 0; continuation < MAX_WINDOWS_PER_SOURCE; continuation++) {
      const id = continuation ? requestId(assignment.job.id, window.source.source_id, `${window.source.revision}#${continuation}`) : window.id;
      const result = await runFormation(host, assignment, session, { ...window, id, operation: input.operation }, judgement, context);
      results.push(result);
      log(`job ${assignment.job.id} source ${window.source.source_id}@${window.source.revision} window ${continuation + 1}: retained ${result.records.length}, deferred ${result.deferred.length}${result.remaining ? ", more remains" : ""}`);
      if (!result.remaining) break;
    }
  }
  const coverage = { examined: results.flatMap((r) => r.coverage.examined), unexamined: results.flatMap((r) => r.coverage.unexamined) };
  // Unexamined material is itself unresolved work: a partial result must say what remains.
  const unresolved = [...new Set([...results.flatMap((r) => r.unresolved), ...coverage.unexamined.map((item) => `Unexamined: ${item}`)])];
  const usage = results.at(-1)?.usage ?? { status: "unknown" as const, input_tokens: null, output_tokens: null, cost: null };
  const completion: WorkResult = {
    schema_version: "1",
    status: unresolved.length || coverage.unexamined.length ? "partial" : "complete",
    examined_scope: brief.scope,
    inputs: brief.inputs,
    findings: [],
    coverage,
    unresolved_work: unresolved,
    proposed_changes: [],
    child_outputs: [],
    known_effects: [],
    usage,
  };
  const committed: HostResponse = await host.request(
    { action: "complete", request_id: requestId(assignment.job.id, "complete", String(assignment.job.attempt)), fence, result: completion },
    context.abortSignal,
  );
  log(`job ${assignment.job.id} ${committed.kind === "job" ? committed.job.state : committed.kind}`);
}
