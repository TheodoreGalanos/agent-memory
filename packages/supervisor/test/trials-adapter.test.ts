// ABOUTME: Unit tests for the qualification trial-execution adapter: an advisory arm answers through a
// ABOUTME: bounded task-model call in this Node process, and an executable arm needs the asp sandbox service.
import { expect, it } from "vitest";
import { BACKGROUND_CONTEXT as context } from "@earendil-works/pi-agent-core";
import { createModels, fauxProvider, fauxAssistantMessage } from "@earendil-works/pi-ai";
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type { QualificationTask } from "../../consolidation/src/qualification.js";
import { trialExecutor } from "../src/adapters/trials.js";
import { input } from "../../pi-worker/test/fixtures.js";

function assignment(): Assignment {
  const brief = input("before-correction").command.payload;
  return {
    job: {
      id: "01990000-0000-7000-8000-000000000001", state: "running", spec: { brief, parent_id: null, max_attempts: 1, retain_until: new Date(Date.now() + 3_600_000).toISOString() },
      root_id: "01990000-0000-7000-8000-000000000001", depth: 0, attempt: 1, session_id: "task:test", operation_id: "01990000-0000-7000-8000-000000000002",
      deadline: new Date(Date.now() + 3_600_000).toISOString(), cancel_requested: false, wait_reason: null, ready_at: null, result: null, composition: null,
    },
    owner_id: "01990000-0000-7000-8000-000000000003", epoch: 1, expires_at: new Date(Date.now() + 3_600_000).toISOString(),
  } as Assignment;
}

const episodesTask: QualificationTask = { operationId: "op-1", arm: "episodes", task: { instance: {}, type: { rated_pressure: 24 } }, representation: [] };

it("answers an advisory trial through the task model, running in this Node process", async () => {
  const faux = fauxProvider(), models = createModels();
  models.setProvider(faux.provider);
  faux.setResponses([fauxAssistantMessage(JSON.stringify({ answer: JSON.stringify({ value: 24, location: "type" }), trajectory: ["Read type.rated_pressure"] }))]);
  const execute = trialExecutor(undefined as never, assignment(), { taskModel: { models, model: faux.getModel(), maxPayloadBytes: 100_000 } });
  const observed = await execute(episodesTask, context);
  expect(observed.answer).toEqual({ value: 24, location: "type" });
  expect(observed.trajectory).toEqual(["Read type.rated_pressure"]);
  expect(observed.cost_microunits).toBe(0);
  expect(Number.isInteger(observed.cost_microunits)).toBe(true);
});

it("rejects an advisory trial when no task model is installed", async () => {
  const execute = trialExecutor(undefined as never, assignment(), {});
  await expect(execute(episodesTask, context)).rejects.toThrow(/task model/);
});

it("rejects a malformed advisory reply rather than guessing its shape", async () => {
  const faux = fauxProvider(), models = createModels();
  models.setProvider(faux.provider);
  faux.setResponses([fauxAssistantMessage(JSON.stringify({ answer: 42, trajectory: "not an array" }))]);
  const execute = trialExecutor(undefined as never, assignment(), { taskModel: { models, model: faux.getModel(), maxPayloadBytes: 100_000 } });
  await expect(execute(episodesTask, context)).rejects.toThrow(/expected shape/);
});

it("rejects an executable candidate trial when no asp execution service is installed", async () => {
  const candidateTask: QualificationTask = {
    operationId: "op-2", arm: "procedure",
    task: { instance: {}, type: { rated_pressure: 24 } },
    representation: {
      version_id: "01990000-0000-7000-8000-000000000004", created_by: "01990000-0000-7000-8000-000000000005", decision_id: "01990000-0000-7000-8000-000000000006",
      recorded: { recorded_at: new Date().toISOString(), sequence: 1 }, recorded_until: null,
      reference: { memory_id: "01990000-0000-7000-8000-000000000007", revision: 1, label: "Candidate" },
      record: {
        label: "Candidate", scope: input("before-correction").command.payload.scope, origin: "agent_generated", evidential_status: "inference",
        availability: "routine", qualification: { status: "candidate" }, valid_time: { kind: "unknown" }, source_locators: [], derived_from: [],
        content: {
          family: "procedure", purpose: "Find rated pressure", capabilities: ["python"], applicability: [], exclusions: [], counterexamples: [],
          method: { form: "executable", artifact_id: "01990000-0000-7000-8000-000000000008", entrypoint: "find_pressure", inputs: ["instance", "type"], outputs: ["value", "location"] },
        },
        decision: { policy: input("before-correction").command.payload.policy, action: "retain", reason: "test", constraints: [], required_evidence: [], expires_at: null },
      },
    },
  };
  const execute = trialExecutor(undefined as never, assignment(), {});
  await expect(execute(candidateTask, context)).rejects.toThrow(/asp execution service/);
  const nodeExecute = trialExecutor(undefined as never, assignment(), { execution: { kind: "node" } });
  await expect(nodeExecute(candidateTask, context)).rejects.toThrow(/asp execution service/);
});
