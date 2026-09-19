// ABOUTME: Formation process handler for the pool: derives one bounded capture window per input source
// ABOUTME: from the assignment, runs each under the runner's lease, and publishes the combined WorkResult.
import { createHash } from "node:crypto";
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type { FormationResult } from "../../../contracts/generated/formation-result.js";
import type { FormationWindow } from "../../../contracts/generated/formation-window.js";
import type { HostResponse } from "../../../contracts/generated/host-response.js";
import type { WorkResult } from "../../../contracts/generated/work-result.js";
import type { JudgementProvider } from "../../judgement/src/providers.js";
import type { JudgementOptions } from "../../judgement/src/runtime.js";
import { runFormation } from "../../formation/src/index.js";
import type { ProcessHandler, ProcessRun } from "./runner.js";
import { observeProvider } from "./trace.js";

export interface FormationDeps {
  judgement: {
    reference: JudgementProvider;
    jev?: JudgementProvider;
    mode?: JudgementOptions["mode"];
    maximum: JudgementProvider["maximum"];
  };
}

/** The persisted work a formation attempt needs, derived deterministically from its assignment so a
 * retried attempt resumes the same windows instead of paying again. */
export interface FormationInput {
  /** The brief's purpose names the operation: the same purpose continues a source's cursor, a new
   * purpose explicitly reinterprets the source. */
  operation: string;
  windows: { id: string; source: FormationWindow["source"]; limit: number }[];
}

export function formationInput(assignment: Assignment): FormationInput {
  const brief = assignment.job.spec.brief;
  if (brief.process !== "formation") throw new Error("Formation input needs a formation assignment");
  return {
    operation: brief.purpose.trim().slice(0, 200) || "capture",
    windows: brief.inputs.sources.map((source) => ({ id: windowId(assignment.job.id, source), source, limit: 32 })),
  };
}

/** A UUID derived from the job and a source so retries reuse the same window or request id. */
export function windowId(jobId: string, source: { source_id: string; revision: string }): string {
  const digest = createHash("sha256").update(`${jobId}:${source.source_id}:${source.revision}`).digest();
  digest[6] = (digest[6] & 0x0f) | 0x80;
  digest[8] = (digest[8] & 0x3f) | 0x80;
  const hex = digest.subarray(0, 16).toString("hex");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20, 32)}`;
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
  if (assignment.job.state !== "running") await host.request({ action: "start", fence }, context.abortSignal);
  const judgement: JudgementOptions = {
    reference: observeProvider(deps.judgement.reference, "reference", log),
    jev: deps.judgement.jev ? observeProvider(deps.judgement.jev, "shadow", log) : undefined,
    mode: deps.judgement.mode,
    maxAttempts: 1,
    disclosure: {
      policy: brief.disclosure_policy,
      scope: brief.scope,
      providers: [deps.judgement.reference.id, ...(deps.judgement.jev ? [deps.judgement.jev.id] : [])],
    },
  };
  const results: FormationResult[] = [];
  for (const window of input.windows) {
    const result = await runFormation(host, assignment, session, { ...window, operation: input.operation }, judgement, context);
    results.push(result);
    log(`job ${assignment.job.id} source ${window.source.source_id}@${window.source.revision}: retained ${result.records.length}, deferred ${result.deferred.length}`);
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
    { action: "complete", request_id: windowId(assignment.job.id, { source_id: "complete", revision: String(assignment.job.attempt) }), fence, result: completion },
    context.abortSignal,
  );
  log(`job ${assignment.job.id} ${committed.kind === "job" ? committed.job.state : committed.kind}`);
}
