import { it, expect } from "vitest";
import { randomUUID } from "node:crypto";
import { catalogue } from "../src/catalogue.js";
import { validateResponse } from "../src/validation.js";
import { decide, qualified } from "../src/policy.js";
import { fixtures, packet, reply, assessment } from "./fixtures.js";

it.each(fixtures)(
  "$family has typed answers, missing-evidence disposition and service-failure fallback",
  (f) => {
    const p = packet(f.family),
      a = assessment(p);
    expect(validateResponse(p, reply(p), true).answers).toEqual(a.answers);
    const normal = decide(p, randomUUID(), [a], a, "reference");
    expect(normal.decisions[f.family].action).toBe(f.expected_action);
    expect(normal.decisions[f.family].reason).not.toContain("unavailable");
    expect(normal.decisions[f.family].constraints.join(" ")).toContain(
      "receipts",
    );
    a.answers = validateResponse(p, reply(p, true), true).answers;
    expect(
      decide(p, randomUUID(), [a], a, "reference").decisions[f.family].action,
    ).toBe("retrieve_further");
    a.status = "invalid_response";
    a.answers = {};
    a.failure = "Wrong answer type";
    expect(
      decide(p, randomUUID(), [a], undefined, "reference").decisions[f.family]
        .action,
    ).toBe("defer");
    expect(p.questions[0].definition.fallback).toContain("conventional-model");
  },
);
it("defines 29 families with explicit field paths and separates overlapping properties", () => {
  expect(catalogue).toHaveLength(29);
  for (const d of catalogue)
    for (const q of d.questions) expect(q.instructions).toContain("`evidence.");
  expect(
    packet("J19").questions[0].definition.questions.map((q) => q.key),
  ).toEqual([
    "conditions",
    "uncertainty",
    "conflicts",
    "obligations",
    "checks",
  ]);
});
it.each(["missing", "wrong_type", "probability", "sum", "winner", "model"])(
  "rejects invalid %s independently of missing evidence",
  (fault) => {
    const p = packet(),
      r = reply(p) as any;
    if (fault === "missing") delete r.answers["J01.assessment"];
    if (fault === "wrong_type") r.answers["J01.assessment"].type = "noul";
    if (fault === "probability")
      r.answers["J01.assessment"].probabilities.inference = NaN;
    if (fault === "sum")
      r.answers["J01.assessment"].probabilities.inference = 0.2;
    if (fault === "winner") r.answers["J01.assessment"].choice = "observation";
    if (fault === "model") delete r.model;
    expect(() => validateResponse(p, r, true)).toThrow();
  },
);
it("validates Noul without manufactured confidence and Score against its ordered rubric", () => {
  const p = packet();
  p.questions[0].definition.questions = [
    {
      key: "truth",
      instructions: "Is the statement supported?",
      form: {
        type: "noul",
        criteria: { true: "Supported", false: "Not supported" },
      },
    },
    {
      key: "level",
      instructions: "How much support?",
      form: { type: "score", criteria: ["None", "Partial", "Full"] },
    },
  ];
  const raw = {
    model: "test",
    answers: {
      "J01.truth": { type: "noul", noul: 0.5 },
      "J01.level": {
        type: "score",
        score: 1.5,
        legend: { "0": "None", "1": "Partial", "2": "Full" },
        probabilities: { "0": 0, "1": 0.5, "2": 0.5 },
        confidence: 0.3,
      },
    },
  };
  expect(validateResponse(p, raw, true).answers["J01.truth"]).toEqual({
    type: "noul",
    noul: 0.5,
  });
  expect(() =>
    validateResponse(
      p,
      {
        ...raw,
        answers: {
          ...raw.answers,
          "J01.truth": { type: "noul", noul: 0.5, confidence: 0.8 },
        },
      },
      true,
    ),
  ).toThrow("Noul");
  for (const score of [1.505, 2]) {
    raw.answers["J01.level"].score = score;
    expect(() => validateResponse(p, raw, true)).toThrow("weighted");
  }
});
it.each([0.99, 1, 1.01])(
  "accepts a distribution total of %s without rescaling",
  (sum) => {
    const p = packet(),
      r = reply(p);
    const answer = r.answers["J01.assessment"];
    answer.probabilities.inference = 0.9;
    answer.probabilities.observation = sum - 0.9;
    expect(validateResponse(p, r, true).answers["J01.assessment"]).toEqual(
      answer,
    );
  },
);
it.each([0.989999, 1.010001, 0.98, 1.02])(
  "rejects a distribution total of %s",
  (sum) => {
    const p = packet(),
      r = reply(p);
    r.answers["J01.assessment"].probabilities.inference = 0.9;
    r.answers["J01.assessment"].probabilities.observation = sum - 0.9;
    expect(() => validateResponse(p, r, true)).toThrow("not normalised");
  },
);
it("keeps the highest-choice tolerance independent of sum rounding", () => {
  const p = packet(),
    r = reply(p);
  r.answers["J01.assessment"].probabilities.inference = 0.497;
  r.answers["J01.assessment"].probabilities.observation = 0.503;
  expect(() => validateResponse(p, r, true)).toThrow("highest-probability");
});
it("requires family, revision, release, scope and evaluated probability for Jev use", () => {
  const p = packet(),
    a = assessment(p),
    gate = {
      family: "J01",
      revision: 2,
      modelRelease: "test-release",
      scope: p.scope,
      evaluationArtifact: randomUUID(),
      minimumProbability: 0.95,
    };
  expect(qualified(p, a, [])).toBe(false);
  expect(qualified(p, a, [gate])).toBe(true);
  expect(qualified(p, a, [{ ...gate, revision: 3 }])).toBe(false);
  a.model_release = null;
  expect(qualified(p, a, [gate])).toBe(false);
});

it("flags policy-declared inconsistencies without treating all related answers as interchangeable", async () => {
  const { inconsistencies } = await import("../src/policy.js");
  const p = packet("J01"),
    a = assessment(p);
  const rule = {
    id: "inference presented as observation",
    allOf: [
      { question: "J01.assessment", choice: "inference" },
      { question: "J01.faithfulness", choice: "no" },
    ],
  };
  expect(inconsistencies(a, [rule])).toEqual([]);
  a.answers["J01.faithfulness"] = { type: "choice", choice: "no" };
  expect(inconsistencies(a, [rule])).toEqual([rule.id]);
});
