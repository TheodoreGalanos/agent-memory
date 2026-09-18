import type { Assignment } from "../../../contracts/generated/assignment.js";
import type {
  JudgementDefinition,
  JudgementEvidence,
  JudgementPacket,
  PacketQuestion,
} from "../../../contracts/generated/judgement-packet.js";
import type { TaskLocalCheck } from "../../../contracts/generated/task-local-check.js";
import type { HostCommands } from "../../pi-worker/src/host-client.js";
import { inspectJson } from "../../pi-worker/src/scoped-operation.js";
import { permits } from "../../pi-worker/src/workspace.js";
import {
  BACKGROUND_CONTEXT,
  type Context,
} from "@earendil-works/pi-agent-core";

export const fenceFor = (assignment: Assignment) => ({
  job_id: assignment.job.id,
  owner_id: assignment.owner_id,
  epoch: assignment.epoch,
});
export interface PacketInput {
  id: string;
  subject: string;
  frame: string;
  fields: Omit<JudgementEvidence, "content">[];
  missing?: string[];
  questions: PacketQuestion[];
  /** Trusted caller's disclosure grant, never supplied by a model. */
  allowedProviders: string[];
  freshAfter: string;
  expiresAt: string;
  localCheckId?: string;
}
export async function buildPacket(
  host: HostCommands,
  assignment: Assignment,
  input: PacketInput,
  context: Context = BACKGROUND_CONTEXT,
): Promise<JudgementPacket> {
  const brief = assignment.job.spec.brief;
  if (
    !input.fields.length ||
    input.fields.length > 32 ||
    !input.questions.length ||
    input.questions.length > 32
  )
    throw new Error("Select a bounded evidence and question batch");
  const ids = new Set(input.questions.map((q) => q.definition.id));
  if (
    ids.size !== input.questions.length ||
    input.questions.some((q) => q.depends_on.some((id) => ids.has(id)))
  )
    throw new Error("Dependent or duplicate families require separate packets");
  for (const question of input.questions) {
    if (
      !permits(question.disclosure_scope, brief.scope) ||
      input.allowedProviders.some(
        (p) => !question.allowed_providers.includes(p),
      )
    )
      throw new Error(
        "Every question must permit disclosure of the whole shared state",
      );
  }
  if (new Set(input.fields.map((f) => f.name)).size !== input.fields.length)
    throw new Error("Evidence field names must be unique");
  const evidence: JudgementEvidence[] = [];
  for (const field of input.fields)
    evidence.push({
      ...field,
      content: (await inspectJson(
        host,
        fenceFor(assignment),
        field.artifact_id,
        field.pointer,
        context,
      )) as JudgementEvidence["content"],
    });
  if (
    input.questions.some((q) =>
      q.depends_on.some((name) => !evidence.some((e) => e.name === name)),
    )
  )
    throw new Error(
      "Dependent questions require inspected results from an earlier packet",
    );
  const missing = new Set(input.missing ?? []);
  for (const q of input.questions)
    for (const requirement of q.definition.input_requirements)
      if (!evidence.some((e) => e.name === requirement))
        missing.add(requirement);
  let deadline = assignment.job.deadline;
  if (input.localCheckId) {
    const check = await host.request(
      {
        action: "inspect_task_check",
        fence: fenceFor(assignment),
        id: input.localCheckId,
      },
      context.abortSignal,
    );
    if (check.kind !== "task_check")
      throw new Error("Task-local check is unavailable");
    if (Date.parse(check.check.expires_at) < Date.parse(deadline))
      deadline = check.check.expires_at;
  }
  const packet: JudgementPacket = {
    id: input.id,
    job_id: assignment.job.id,
    subject: input.subject,
    frame: input.frame,
    scope: brief.scope,
    inputs: {
      sources: [],
      memories: [],
      artifacts: [...new Set(input.fields.map((f) => f.artifact_id))],
    },
    evidence,
    missing: [...missing],
    questions: input.questions,
    disclosure_policy: brief.disclosure_policy,
    allowed_providers: input.allowedProviders,
    policy: brief.policy,
    budget_id: brief.limits.root_budget_id,
    evidence_cutoff: brief.evidence_cutoff,
    fresh_after: input.freshAfter,
    deadline,
    expires_at: input.expiresAt,
    local_check_id: input.localCheckId,
  };
  const admitted = await host.request(
    { action: "admit_judgement", fence: fenceFor(assignment), packet },
    context.abortSignal,
  );
  if (admitted.kind !== "judgement_packet")
    throw new Error("Host did not admit the judgement packet");
  return admitted.packet;
}

export async function createTaskCheck(
  host: HostCommands,
  assignment: Assignment,
  id: string,
  definition: JudgementDefinition,
  allowedProviders: string[],
  expiresAt: string,
): Promise<TaskLocalCheck> {
  const brief = assignment.job.spec.brief;
  if (definition.permitted_uses.some((use) => use !== "investigation"))
    throw new Error("Task-local checks cannot acquire effect permissions");
  const response = await host.request({
    action: "register_task_check",
    fence: fenceFor(assignment),
    check: {
      id,
      job_id: assignment.job.id,
      task_id: brief.task_id,
      definition,
      scope: brief.scope,
      allowed_providers: allowedProviders,
      disclosure_policy: brief.disclosure_policy,
      completion_requirements: brief.output_criteria,
      expires_at: expiresAt,
      retired: false,
    },
  });
  if (response.kind !== "task_check")
    throw new Error("Host did not register the check");
  return response.check;
}
