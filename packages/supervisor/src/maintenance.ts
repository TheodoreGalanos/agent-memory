// ABOUTME: Maintenance process module for the pool: a job's manifest artifact names either a correction to
// ABOUTME: apply and propagate or an intention transition to check; the handler runs the one the brief names.
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type { IntentionCheckKind } from "../../../contracts/generated/intention-check.js";
import type { MaintenanceRequest } from "../../../contracts/generated/maintenance-review.js";
import type { HostResponse } from "../../../contracts/generated/host-response.js";
import type { WorkResult } from "../../../contracts/generated/work-result.js";
import type { Context } from "@earendil-works/pi-agent-core";
import type { JudgementOptions } from "../../judgement/src/runtime.js";
import { catalogueResponses } from "../../judgement/src/responses.js";
import { inspectJson } from "../../host-client/src/artifact.js";
import { fenceFor } from "../../host-client/src/fence.js";
import type { HostCommands } from "../../host-client/src/client.js";
import { runMaintenance, runIntentionCheck } from "../../maintenance/src/index.js";
import type { ProcessModule, Services } from "./composition.js";
import type { ProcessHandler, ProcessRun } from "./runner.js";
import { windowId } from "./formation.js";
import { observeProvider } from "./trace.js";

type MaintenanceDeps = Required<Pick<Services, "judgement">>;

/** Maintenance needs the judgement providers; it drives no task model. */
export const maintenanceModule: ProcessModule = {
  declaration: { id: "maintenance", version: "1", requires: ["judgement"], operations: ["maintenance"] },
  attach: (services) => [maintenanceHandler({ judgement: services.judgement! })],
};

/** What a maintenance job's manifest artifact (the brief's first input artifact) names: a correction to
 * apply and propagate, or an intention occurrence transition to check. Lane B's Host-timer scheduler is
 * expected to publish this manifest when it schedules the job, mirroring how qualification already
 * carries its manifest in `inputs.artifacts[0]` (see `packages/consolidation/src/qualification.ts`). */
export type MaintenanceManifest =
  | { kind: "correction"; id: string; request: MaintenanceRequest }
  | { kind: "intention_check"; id: string; occurrence_id: string; check: IntentionCheckKind };

function validateManifest(raw: unknown): MaintenanceManifest {
  if (!raw || typeof raw !== "object") throw new Error("Maintenance manifest must be an object");
  const m = raw as Record<string, unknown>;
  if (m.kind === "correction") {
    if (typeof m.id !== "string" || !m.request || typeof m.request !== "object") throw new Error("Correction manifest needs an id and a request");
    return { kind: "correction", id: m.id, request: m.request as MaintenanceRequest };
  }
  if (m.kind === "intention_check") {
    if (typeof m.id !== "string" || typeof m.occurrence_id !== "string" || !m.check || typeof m.check !== "object")
      throw new Error("Intention check manifest needs an id, an occurrence_id and a check kind");
    return { kind: "intention_check", id: m.id, occurrence_id: m.occurrence_id, check: m.check as IntentionCheckKind };
  }
  throw new Error(`Unknown maintenance manifest kind ${String(m.kind)}`);
}

/** The manifest a maintenance job names, derived deterministically from its assignment so a retried
 * attempt resumes the same correction or intention check instead of paying for a new one. */
export async function maintenanceInput(host: HostCommands, assignment: Assignment, context: Context): Promise<MaintenanceManifest> {
  const brief = assignment.job.spec.brief;
  if (brief.process !== "maintenance") throw new Error("Maintenance input needs a maintenance assignment");
  const manifestId = brief.inputs.artifacts[0];
  if (!manifestId) throw new Error("Maintenance needs a manifest artifact naming the correction or intention check to run");
  return validateManifest(await inspectJson(host, fenceFor(assignment), manifestId, "", context));
}

export function maintenanceHandler(deps: MaintenanceDeps): ProcessHandler<"maintenance"> {
  return { process: "maintenance", run: (run) => runMaintenanceJob(run, deps) };
}

async function runMaintenanceJob(run: ProcessRun, deps: MaintenanceDeps) {
  const { host, lease, session, context, log } = run;
  const assignment = lease.assignment;
  const brief = assignment.job.spec.brief;
  const fence = lease.fence;
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
  let coverage = { examined: [] as string[], unexamined: [] as string[] };
  let unresolved: string[] = [];
  let childOutputs: string[] = [];
  try {
    const manifest = await maintenanceInput(host, assignment, context);
    if (manifest.kind === "correction") {
      const result = await runMaintenance(host, assignment, session, { id: manifest.id, request: manifest.request }, judgement, context);
      coverage = result.coverage;
      unresolved = result.unresolved;
      const published = await host.request(
        { action: "publish_artifact", fence, request_id: windowId(assignment.job.id, { source_id: "maintenance-result", revision: String(assignment.job.attempt) }), label: "Maintenance correction result", text: JSON.stringify(result), dependencies: [] },
        context.abortSignal,
      );
      if (published.kind !== "artifact") throw new Error("Maintenance result was not published");
      childOutputs = [published.artifact.id];
      log(`job ${assignment.job.id} maintenance ${result.kind}: ${result.changed.length} changed, ${result.revalidation.length} flagged for revalidation`);
    } else {
      const occurrence = await runIntentionCheck(host, assignment, session, { id: manifest.id, occurrence_id: manifest.occurrence_id, kind: manifest.check }, judgement, context);
      coverage = { examined: [`Intention occurrence ${occurrence.id}: ${occurrence.state}`], unexamined: [] };
      const published = await host.request(
        { action: "publish_artifact", fence, request_id: windowId(assignment.job.id, { source_id: "intention-check-result", revision: String(assignment.job.attempt) }), label: "Intention check result", text: JSON.stringify(occurrence), dependencies: [] },
        context.abortSignal,
      );
      if (published.kind !== "artifact") throw new Error("Intention check result was not published");
      childOutputs = [published.artifact.id];
      log(`job ${assignment.job.id} intention check ${manifest.check.kind}: occurrence ${occurrence.id} -> ${occurrence.state}`);
    }
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    unresolved = [reason];
    log(`job ${assignment.job.id} maintenance unavailable: ${reason}`);
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
