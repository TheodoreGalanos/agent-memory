// ABOUTME: Tests for judgement policy: structural outcomes stay distinct from process-owned response rules,
// ABOUTME: every answer form has an admission and a rule builder, and evaluation status is visible but not gating.
import { randomUUID } from "node:crypto";
import { expect, it } from "vitest";
import type { JudgementAnswer } from "../../../contracts/generated/semantic-assessment.js";
import {
  admissible,
  choiceResponse,
  decide,
  evaluationStatus,
  noulResponse,
  qualified,
  scoreResponse,
  type Qualification,
} from "../src/policy.js";
import { assessment, packet } from "./fixtures.js";

const gateFor = (p: ReturnType<typeof packet>, family = "J01"): Qualification => ({
  family, revision: p.questions[0].definition.revision, modelRelease: "test-release", scope: p.scope,
  evaluationArtifact: randomUUID(), minimumProbability: 0.9,
});

it("records unresolved work when no assessment was selected, without interpreting any label", () => {
  const p = packet("J02");
  const unavailable = { ...assessment(p), status: "unavailable" as const, answers: {}, failure: "Provider status 503" };
  const d = decide(p, randomUUID(), [unavailable], undefined, "unresolved");
  expect(d.mode).toBe("unresolved");
  expect(d.selected_assessment).toBeUndefined();
  expect(d.decisions.J02.action).toBe("defer");
  expect(d.decisions.J02.reason).toContain("unavailable");
});

it("treats missing or insufficient evidence as a structural outcome, ahead of any response rule", () => {
  const p = packet("J02");
  const a = assessment(p);
  const rule = { J02: choiceResponse("assessment", { supports: "retain" }, "investigate") };
  expect(decide({ ...p, missing: ["evidence"] }, randomUUID(), [a], a, "primary", rule).decisions.J02).toMatchObject({
    action: "retrieve_further", required_evidence: ["evidence"],
  });
  const insufficient = { ...a, answers: { "J02.assessment": { type: "choice" as const, choice: "insufficient_evidence" } } };
  expect(decide(p, randomUUID(), [insufficient], insufficient, "primary", rule).decisions.J02).toMatchObject({
    action: "retrieve_further", required_evidence: p.questions[0].definition.input_requirements,
  });
  // A task-local check can only ever ask for more work.
  expect(decide({ ...p, local_check_id: randomUUID() }, randomUUID(), [a], a, "primary", rule).decisions.J02.action).toBe("investigate");
});

it("applies the owning process's response rule, and otherwise leaves the assessment for the process to qualify", () => {
  const p = packet("J02");
  const a = { ...assessment(p), answers: { "J02.assessment": { type: "choice" as const, choice: "supports" } } };
  expect(decide(p, randomUUID(), [a], a, "primary").decisions.J02.action).toBe("qualify");
  const rules = { J02: choiceResponse("assessment", { supports: "retain", contradicts: "revise" }, "investigate") };
  expect(decide(p, randomUUID(), [a], a, "primary", rules).decisions.J02.action).toBe("retain");
  const contradicts = { ...a, answers: { "J02.assessment": { type: "choice" as const, choice: "contradicts" } } };
  expect(decide(p, randomUUID(), [contradicts], contradicts, "primary", rules).decisions.J02.action).toBe("revise");
  const other = { ...a, answers: { "J02.assessment": { type: "choice" as const, choice: "other" } } };
  expect(decide(p, randomUUID(), [other], other, "primary", rules).decisions.J02.action).toBe("investigate");
  expect(decide(p, randomUUID(), [a], a, "primary", rules).decisions.J02.reason).toContain("assessment=supports");
});

it("offers Noul and Score response rules alongside Choice", () => {
  const noul = noulResponse("assessment", { atLeast: 0.8, then: "retain", otherwise: "investigate" });
  const definition = packet("J02").questions[0].definition;
  expect(noul({ assessment: { type: "noul", noul: 0.9 } }, definition)).toBe("retain");
  expect(noul({ assessment: { type: "noul", noul: 0.5 } }, definition)).toBe("investigate");
  expect(noul({ assessment: { type: "choice", choice: "supports" } }, definition)).toBe("investigate");
  const score = scoreResponse("assessment", ["defer", "qualify", "retain"], "investigate");
  expect(score({ assessment: { type: "score", score: 2, legend: {} } }, definition)).toBe("retain");
  expect(score({ assessment: { type: "score", score: 0.6, legend: {} } }, definition)).toBe("qualify");
  expect(score({ assessment: undefined }, definition)).toBe("investigate");
});

it("admits each answer form on the probability of what it asserts", () => {
  const choice: JudgementAnswer = { type: "choice", choice: "supports", probabilities: { supports: 0.95, contradicts: 0.05 }, confidence: 0.9 };
  expect(admissible(choice, 0.9)).toBe(true);
  expect(admissible({ ...choice, probabilities: { supports: 0.8, contradicts: 0.2 } }, 0.9)).toBe(false);
  expect(admissible({ type: "choice", choice: "insufficient_evidence", probabilities: { insufficient_evidence: 1 } }, 0.9)).toBe(false);
  // Confidence is distribution sharpness, never a substitute for the selected probability.
  expect(admissible({ type: "choice", choice: "supports", confidence: 1 }, 0.9)).toBe(false);
  expect(admissible({ type: "noul", noul: 0.95 }, 0.9)).toBe(true);
  expect(admissible({ type: "noul", noul: 0.03 }, 0.9)).toBe(true);
  expect(admissible({ type: "noul", noul: 0.6 }, 0.9)).toBe(false);
  const score: JudgementAnswer = { type: "score", score: 2, legend: {}, probabilities: { "0": 0, "1": 0.05, "2": 0.95 }, confidence: 1 };
  expect(admissible(score, 0.9)).toBe(true);
  expect(admissible({ ...score, score: 1.5, probabilities: { "0": 0, "1": 0.5, "2": 0.5 } }, 0.9)).toBe(false);
  expect(admissible({ type: "score", score: 2, legend: {} }, 0.9)).toBe(false);
});

it("reports evaluation status per family without letting it gate selection", () => {
  const p = packet("J01");
  const a = assessment(p);
  const gate = gateFor(p);
  expect(qualified(p, a, [gate])).toBe(true);
  expect(evaluationStatus(p, p.questions[0].definition, a, [gate])).toBe("qualified");
  expect(evaluationStatus(p, p.questions[0].definition, a, [])).toBe("experimental");
  const experimental = decide(p, randomUUID(), [a], a, "primary", undefined, []);
  expect(experimental.selected_assessment).toBe(a.id);
  expect(experimental.decisions.J01.constraints.join(" ")).toMatch(/experimental/);
  const qualifiedDecision = decide(p, randomUUID(), [a], a, "primary", undefined, [gate]);
  expect(qualifiedDecision.decisions.J01.constraints.join(" ")).toMatch(/qualified/);
});
