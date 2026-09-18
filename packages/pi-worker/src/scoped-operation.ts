import { randomUUID } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import {
  value,
  type Context,
  type Session,
} from "@earendil-works/pi-agent-core";
import type {
  Assignment,
  Job,
  WorkResult,
} from "../../../contracts/generated/assignment.js";
import type { InvestigationPlan } from "../../../contracts/generated/investigation-plan.js";
import type { Fence } from "../../../contracts/generated/host-request.js";
import type { HostCommands } from "./host-client.js";
import { permits, type WorkspaceState } from "./workspace.js";
import type { WorkBrief } from "../../../contracts/generated/assignment.js";

interface InvestigationState {
  plan: InvestigationPlan;
  requestIds: string[];
  children: string[];
  definitions?: string[];
  bundleId: string;
}
export type { InvestigationPlan };

/** Bounded inspection of an assigned JSON artifact. No executable queries. */
export async function inspectJson(
  host: HostCommands,
  fence: Fence,
  artifactId: string,
  pointer: string,
  context: Context,
): Promise<unknown> {
  if (pointer !== "" && !pointer.startsWith("/"))
    throw new Error("Expected a JSON pointer");
  const response = await host.request(
    {
      action: "read_input",
      fence,
      artifact_id: artifactId,
      offset: 0,
      limit: 65536,
    },
    context.abortSignal,
  );
  if (response.kind !== "artifact_data")
    throw new Error("Host did not return an assigned artifact");
  let selected: unknown = JSON.parse(response.text);
  for (const encoded of pointer ? pointer.slice(1).split("/") : []) {
    if (/~(?:[^01]|$)/.test(encoded))
      throw new Error("Invalid JSON pointer escape");
    const key = encoded.replace(/~1/g, "/").replace(/~0/g, "~");
    if (
      !selected ||
      typeof selected !== "object" ||
      !Object.hasOwn(selected, key)
    )
      throw new Error(`Required definition is missing at ${pointer}`);
    selected = (selected as Record<string, unknown>)[key];
  }
  return selected;
}
const terminal = (job: Job) =>
  ["completed", "partial", "failed", "cancelled"].includes(job.state);

/** Prepares one bounded decomposition batch. Host jobs schedule independent Pi
 * sessions; this function never starts another driver or copies parent history. */
export async function prepareInvestigation(
  assignment: Assignment,
  host: HostCommands,
  session: Session,
  plan: InvestigationPlan,
  context: Context,
): Promise<
  { kind: "waiting" } | { kind: "ready"; children: Job[]; bundleId: string }
> {
  const job = assignment.job;
  const parent = job.spec.brief;
  const fence = {
    job_id: job.id,
    owner_id: assignment.owner_id,
    epoch: assignment.epoch,
  };
  if (
    !plan.steps.length ||
    plan.steps.length > Math.min(32, parent.limits.max_child_concurrency) ||
    new Set(plan.steps.map((s) => s.key)).size !== plan.steps.length ||
    plan.steps.some(
      (s) => !s.key.trim() || !s.question.trim() || !s.output_criteria.length,
    )
  )
    throw new Error("Invalid or excessive investigation batch");
  const address = value<InvestigationState>(
    "memory.investigation",
    job.operation_id,
  );
  let state = (await session.getValue(address, context))?.value;
  if (state && !isDeepStrictEqual(state.plan, plan))
    throw new Error("The admitted investigation plan changed");
  if (!state) {
    state = {
      plan,
      requestIds: plan.steps.map(() => randomUUID()),
      children: [],
      bundleId: randomUUID(),
    };
    await session.setValue(address, state, context);
  }
  if (!state.definitions) {
    const definitions = [
      ...parent.definitions,
      `Method ${plan.method.id}, revision ${plan.method.revision}: ${plan.method.label}`,
      ...plan.interpretation_conditions,
    ];
    for (const lookup of plan.definitions) {
      const content = await inspectJson(
        host,
        fence,
        lookup.artifact_id,
        lookup.pointer,
        context,
      );
      const text = JSON.stringify(content);
      if (text.length > 2048)
        throw new Error(
          "A required definition exceeds the brief limit; select a narrower field",
        );
      definitions.push(`${lookup.name}: ${text}`);
    }
    state.definitions = definitions;
    await session.setValue(address, state, context);
  }
  const childDeadline = new Date(Date.parse(job.deadline) - 2000).toISOString();
  for (let i = state.children.length; i < plan.steps.length; i++) {
    const step = plan.steps[i];
    const brief = structuredClone(parent);
    brief.purpose = step.question;
    brief.definitions = state.definitions;
    brief.inputs = structuredClone(step.inputs);
    brief.inputs.artifacts = [
      ...new Set([
        ...brief.inputs.artifacts,
        ...plan.definitions.map((d) => d.artifact_id),
      ]),
    ];
    brief.output_criteria = step.output_criteria;
    brief.capabilities.tools = step.tools;
    brief.capabilities.sources = brief.inputs.sources;
    if (brief.inputs.sources.length && brief.scope.source_versions.length)
      brief.scope.source_versions = brief.inputs.sources;
    const response = await host.request(
      {
        action: "spawn_child",
        fence,
        request_id: state.requestIds[i],
        brief,
        deadline: childDeadline,
        reuse_job_id: step.reuse_job_id,
      },
      context.abortSignal,
    );
    if (response.kind !== "job")
      throw new Error("Host did not return a child job");
    state.children.push(response.job.id);
    await session.setValue(address, state, context);
  }
  const response = await host.request(
    { action: "child_jobs", fence, ids: state.children },
    context.abortSignal,
  );
  if (response.kind !== "children")
    throw new Error("Host did not return child results");
  if (!response.jobs.every(terminal)) {
    await host.request(
      {
        action: "wait_children",
        fence,
        ids: state.children,
        ready_at: new Date(Date.parse(job.deadline) - 1000).toISOString(),
      },
      context.abortSignal,
    );
    return { kind: "waiting" };
  }
  const bundle = {
    method: plan.method,
    interpretation_conditions: plan.interpretation_conditions,
    children: response.jobs.map((child, i) => ({
      question: plan.steps[i].question,
      required_outputs: plan.steps[i].output_criteria,
      job_id: child.id,
      operation_id: child.operation_id,
      evidence_cutoff: child.spec.brief.evidence_cutoff,
      applicability: child.spec.brief.scope,
      state: child.state,
      result_artifact: child.result?.result_artifact ?? null,
      coverage: child.result?.coverage ?? null,
    })),
  };
  const published = await host.request(
    {
      action: "publish_artifact",
      fence,
      request_id: state.bundleId,
      label: "Investigation evidence and coverage",
      text: JSON.stringify(bundle),
      dependencies: response.jobs.flatMap((j) =>
        j.result?.result_artifact ? [j.result.result_artifact] : [],
      ),
    },
    context.abortSignal,
  );
  if (published.kind !== "artifact")
    throw new Error("Host did not publish the investigation basis");
  return { kind: "ready", children: response.jobs, bundleId: state.bundleId };
}

/** Child observations remain delegated findings, even when the parent quotes them. */
export function investigationWorkspace(
  workspace: WorkspaceState,
  children: Job[],
  bundleId: string,
): WorkspaceState {
  const next = structuredClone(workspace);
  for (const child of children) {
    const id = `child:${child.id}`;
    if (next.entries.some((e) => e.id === id)) continue;
    const result = child.result;
    next.entries.push({
      ...structuredClone(next.entries[0]),
      id,
      kind: "observation",
      origin: "agent_generated",
      evidential_status: "inference",
      text: JSON.stringify({
        question: child.spec.brief.purpose,
        evidence_cutoff: child.spec.brief.evidence_cutoff,
        status: result?.status ?? child.state,
        findings: result?.findings ?? [],
        coverage: result?.coverage ?? {
          examined: [],
          unexamined: [child.spec.brief.purpose],
        },
        unresolved: result?.unresolved_work ?? [
          "Child produced no validated output",
        ],
        basis_artifact: bundleId,
      }),
      scope: child.spec.brief.scope,
      inputs: result?.inputs ?? child.spec.brief.inputs,
      exposure: "delegated_finding",
      generating_operation: child.operation_id,
      supporting_entries: [],
      decision_relevant: true,
    });
  }
  return next;
}

/** Preserve counterexamples and missing coverage instead of trusting synthesis
 * to repeat every child qualification. Usage remains the host ledger's concern. */
export function preserveChildEvidence(
  result: WorkResult,
  children: Job[],
  bundleId: string,
  brief: WorkBrief,
): WorkResult {
  if (!permits(brief.scope, result.examined_scope))
    throw new Error("Synthesis exceeded the assigned scope");
  const combined = structuredClone(result);
  combined.examined_scope = brief.scope;
  combined.child_outputs = [...new Set([...combined.child_outputs, bundleId])];
  for (const child of children) {
    const output = child.result;
    if (!output) {
      combined.coverage.unexamined.push(child.spec.brief.purpose);
      combined.unresolved_work.push(
        `Child ${child.id} ended ${child.state} without validated output`,
      );
      continue;
    }
    if (output.status !== "complete")
      combined.unresolved_work.push(
        `Child ${child.id} reported ${output.status}`,
      );
    if (output.result_artifact)
      combined.child_outputs.push(output.result_artifact);
    for (const finding of output.findings)
      if (!combined.findings.some((f) => isDeepStrictEqual(f, finding)))
        combined.findings.push(finding);
    for (const key of ["sources", "memories", "artifacts"] as const) {
      // Each array retains its own reference type; no identifiers are coalesced across kinds.
      const target = combined.inputs[key] as unknown[];
      for (const ref of output.inputs[key])
        if (!target.some((r) => isDeepStrictEqual(r, ref))) target.push(ref);
    }
    combined.coverage.examined.push(
      ...output.coverage.examined.map((c) => `${child.id}: ${c}`),
    );
    combined.coverage.unexamined.push(
      ...output.coverage.unexamined.map((c) => `${child.id}: ${c}`),
    );
    combined.unresolved_work.push(
      ...output.unresolved_work.map((c) => `${child.id}: ${c}`),
    );
    combined.known_effects.push(...output.known_effects);
  }
  combined.child_outputs = [...new Set(combined.child_outputs)];
  if (
    combined.coverage.unexamined.length ||
    combined.unresolved_work.length ||
    combined.known_effects.some((e) => e.status === "unknown")
  ) {
    combined.status = "partial";
    if (!combined.unresolved_work.length)
      combined.unresolved_work.push(
        "Child coverage or effects remain unresolved",
      );
  }
  // Allow space for the result artifact ID and host-authoritative usage fields.
  if (
    Buffer.byteLength(JSON.stringify(combined)) + 512 >
    brief.limits.max_output_bytes
  ) {
    combined.status = "partial";
    combined.findings = [];
    combined.proposed_changes = [];
    combined.coverage = {
      examined: [],
      unexamined: [
        "Child findings exceed the publication limit; inspect the referenced child results before drawing conclusions",
      ],
    };
    combined.unresolved_work = [
      `Review the complete findings and coverage through investigation artifact ${bundleId}`,
    ];
    if (
      Buffer.byteLength(JSON.stringify(combined)) + 512 >
      brief.limits.max_output_bytes
    )
      throw new Error(
        "Publication limit cannot hold the investigation references and effects",
      );
  }
  return combined;
}
