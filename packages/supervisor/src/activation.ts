// ABOUTME: Activation process module for the pool: an explicit activation job retrieves relevant memories for
// ABOUTME: another consumer (not a task's own admission, which investigation already runs inline) and publishes
// ABOUTME: the resulting context package as the job's result.
import type { HostResponse } from "../../../contracts/generated/host-response.js";
import type { WorkResult } from "../../../contracts/generated/work-result.js";
import { runActivation } from "../../activation/src/index.js";
import { admissionQuery } from "./investigation.js";
import type { ProcessModule, Services } from "./composition.js";
import type { ProcessHandler, ProcessRun } from "./runner.js";
import { requestId } from "../../host-client/src/identity.js";
import { judgementFor } from "./judgement.js";

type ActivationDeps = Required<Pick<Services, "judgement">>;

/** Activation needs the judgement providers; it drives no task model of its own (retrieval reuses the
 * same admission question shape investigation asks at task admission). */
export const activationModule: ProcessModule = {
  declaration: { id: "activation", version: "1", requires: ["judgement"], operations: ["activation"] },
  attach: (services) => [activationHandler({ judgement: services.judgement! })],
};

export function activationHandler(deps: ActivationDeps): ProcessHandler<"activation"> {
  return { process: "activation", run: (run) => runActivationJob(run, deps) };
}

async function runActivationJob(run: ProcessRun, deps: ActivationDeps) {
  const { host, lease, session, context, log } = run;
  const assignment = lease.assignment;
  const brief = assignment.job.spec.brief;
  const fence = lease.fence;
  if (assignment.job.state !== "running")
    await host.request({ action: "start", fence, composition: run.composition }, context.abortSignal);
  const judgement = judgementFor(deps.judgement, brief, log);
  // No task model bounds this job's payload; a conservative fixed allowance keeps the retrieval question small.
  const maxPayloadBytes = Math.min(60_000, brief.limits.max_output_bytes);
  let coverage = { examined: [] as string[], unexamined: [] as string[] };
  let unresolved: string[] = [];
  let childOutputs: string[] = [];
  try {
    const query = admissionQuery(assignment, maxPayloadBytes);
    const id = requestId(assignment.job.id, "activation", String(assignment.job.attempt));
    const result = await runActivation(host, assignment, session, { id, query }, judgement, context);
    coverage = { examined: result.selected.map((r) => `${r.label} r${r.revision}`), unexamined: result.deferred };
    unresolved = result.deferred;
    const published = await host.request(
      { action: "publish_artifact", fence, request_id: requestId(assignment.job.id, "activation-result", String(assignment.job.attempt)), label: "Activation context package", text: JSON.stringify(result), dependencies: [] },
      context.abortSignal,
    );
    if (published.kind !== "artifact") throw new Error("Activation result was not published");
    childOutputs = [published.artifact.id];
    log(`job ${assignment.job.id} activation: ${result.selected.length} memory(ies) selected, ${result.deferred.length} deferred`);
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    unresolved = [reason];
    log(`job ${assignment.job.id} activation unavailable: ${reason}`);
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
    { action: "complete", request_id: requestId(assignment.job.id, "complete", String(assignment.job.attempt)), fence, result: completion },
    context.abortSignal,
  );
  log(`job ${assignment.job.id} ${committed.kind === "job" ? committed.job.state : committed.kind}`);
}
