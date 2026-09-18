import { randomUUID } from "node:crypto";
import {
  value,
  type Context,
  type Session,
} from "@earendil-works/pi-agent-core";
import type {
  Assignment,
  WorkResult,
} from "../../../contracts/generated/assignment.js";
import type {
  QualificationInput,
  MemoryVersion,
} from "../../../contracts/generated/qualification-input.js";
import type { QualificationSuite } from "../../../contracts/generated/qualification-suite.js";
import type {
  QualificationArm,
  QualificationReport,
  QualificationTrial,
  RecognitionObservation,
} from "../../../contracts/generated/qualification-report.js";
import type { HostCommands } from "../../pi-worker/src/host-client.js";
import { inspectJson } from "../../pi-worker/src/scoped-operation.js";
import { fenceFor } from "../../judgement/src/packet.js";
import type { InterpreterClient } from "../../pi-worker/src/interpreter-client.js";

export interface QualificationExecution {
  answer: unknown;
  trajectory: unknown;
  cost_microunits: number;
  recognition: RecognitionObservation[];
}
export interface QualificationTask {
  operationId: string;
  arm: QualificationArm;
  task: unknown;
  representation: MemoryVersion[] | string | MemoryVersion;
}
interface State {
  inputId: string;
  operations: Record<string, string>;
  trials: QualificationTrial[];
  reportId: string;
}
/** Run with the assigned lease keeper. `execute` uses the existing guarded harness;
 * protected expected answers and source-group labels never enter its task input. */
export async function runQualification(
  host: HostCommands,
  assignment: Assignment,
  session: Session,
  execute: (
    task: QualificationTask,
    context: Context,
  ) => Promise<QualificationExecution>,
  context: Context,
): Promise<WorkResult> {
  if (assignment.job.spec.brief.process !== "evaluation")
    throw new Error("Qualification needs an evaluation assignment");
  const fence = fenceFor(assignment),
    inputId = assignment.job.spec.brief.inputs.artifacts[0];
  if (!inputId) throw new Error("Evaluation needs a qualification manifest");
  const input = (await inspectJson(
    host,
    fence,
    inputId,
    "",
    context,
  )) as QualificationInput;
  const suite = (await inspectJson(
    host,
    fence,
    input.suite_id,
    "",
    context,
  )) as QualificationSuite;
  if (!suite.cases.length || suite.cases.length > 32)
    throw new Error("Qualification suite exceeds its bound");
  const key = value<State>("memory.qualification", assignment.job.operation_id);
  let state = (await session.getValue(key, context))?.value;
  if (state && state.inputId !== inputId)
    throw new Error("Evaluation inputs changed");
  if (!state) {
    state = { inputId, operations: {}, trials: [], reportId: randomUUID() };
    await session.setValue(key, state, context);
  }
  for (const c of suite.cases)
    for (const arm of ["episodes", "summary", "procedure"] as const) {
      if (state.trials.some((t) => t.case_id === c.id && t.arm === arm))
        continue;
      context.abortSignal?.throwIfAborted();
      const trialKey = `${c.id}/${arm}`;
      const operationId = state.operations[trialKey] ?? randomUUID();
      state.operations[trialKey] = operationId;
      await session.setValue(key, state, context);
      const observed = await execute(
        {
          operationId,
          arm,
          task: structuredClone(c.task),
          representation:
            arm === "episodes"
              ? input.review.window.cases
              : arm === "summary"
                ? input.review.proposal.concise_summary
                : input.candidate,
        },
        context,
      );
      if (
        !Number.isInteger(observed.cost_microunits) ||
        observed.cost_microunits < 0
      )
        throw new Error("Evaluation must report known nonnegative cost");
      const published = await host.request(
        {
          action: "publish_artifact",
          fence,
          request_id: operationId,
          label: "Qualification trial evidence",
          text: JSON.stringify(observed),
          dependencies: [inputId],
        },
        context.abortSignal,
      );
      if (published.kind !== "artifact")
        throw new Error("Trial evidence was not published");
      state.trials.push({
        case_id: c.id,
        arm,
        answer: observed.answer as QualificationTrial["answer"],
        evidence_artifact: published.artifact.id,
        cost_microunits: observed.cost_microunits,
        recognition: observed.recognition,
      });
      await session.setValue(key, state, context);
    }
  const report: QualificationReport = {
    review_id: input.review.id,
    candidate: input.candidate.reference,
    suite_id: input.suite_id,
    evaluator_profile: suite.evaluator_profile,
    trials: state.trials,
    unresolved: [],
  };
  const published = await host.request(
    {
      action: "publish_artifact",
      fence,
      request_id: state.reportId,
      label: "Procedure transfer comparison",
      text: JSON.stringify(report),
      dependencies: [
        inputId,
        input.suite_id,
        ...state.trials.map((t) => t.evidence_artifact),
      ],
    },
    context.abortSignal,
  );
  if (published.kind !== "artifact")
    throw new Error("Qualification report was not published");
  const result: WorkResult = {
    schema_version: "1",
    status: "complete",
    examined_scope: assignment.job.spec.brief.scope,
    inputs: assignment.job.spec.brief.inputs,
    findings: [],
    coverage: { examined: suite.cases.map((c) => c.id), unexamined: [] },
    unresolved_work: [],
    proposed_changes: [],
    child_outputs: [published.artifact.id],
    result_artifact: null,
    known_effects: [],
    usage: {
      status: "unknown",
      input_tokens: null,
      output_tokens: null,
      cost: null,
    },
  };
  return result;
}

/** Candidate code runs only in the assigned WP07 interpreter with its effect receipts. */
export async function executeProcedure(
  host: HostCommands,
  assignment: Assignment,
  interpreter: InterpreterClient,
  candidate: MemoryVersion,
  operationId: string,
  inputs: Record<string, unknown>,
  context: Context,
): Promise<unknown> {
  const procedure = candidate.record.content;
  if (
    procedure.family !== "procedure" ||
    procedure.method.form !== "executable" ||
    !procedure.contract
  )
    throw new Error("Expected an executable procedure with a contract");
  if (
    !procedure.capabilities.every((c) =>
      assignment.job.spec.brief.capabilities.tools.includes(c),
    ) ||
    procedure.contract.replay_class !== "reconcile"
  )
    throw new Error(
      "Procedure execution exceeds assigned capabilities or replay policy",
    );
  if (
    !/^[A-Za-z_][A-Za-z0-9_]*$/.test(procedure.method.entrypoint) ||
    procedure.method.inputs.some((key) => !(key in inputs))
  )
    throw new Error("Procedure entrypoint or required inputs are invalid");
  const read = await host.request(
    {
      action: "read_input",
      fence: fenceFor(assignment),
      artifact_id: procedure.method.artifact_id,
      offset: 0,
      limit: 32768,
    },
    context.abortSignal,
  );
  if (read.kind !== "artifact_data" || Buffer.byteLength(read.text) > 16384)
    throw new Error("Procedure code is unavailable or exceeds 16 KiB");
  const code = `import json\nexec(${JSON.stringify(read.text)})\nprint(json.dumps(${procedure.method.entrypoint}(**json.loads(${JSON.stringify(JSON.stringify(inputs))}))))`;
  const result = await interpreter.execute(operationId, code, context);
  if (result.status !== "ok")
    throw new Error(result.error ?? "Procedure execution failed");
  return JSON.parse(result.output ?? "");
}
