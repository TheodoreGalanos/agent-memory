// ABOUTME: Tests for the packet planner: independent questions sharing evidence, disclosure and timing form
// ABOUTME: one packet; dependents wait for the wave that produces their inputs; bounds are respected.
import { expect, it } from "vitest";
import { definition } from "../src/catalogue.js";
import { planPackets, type PlannedQuestion } from "../src/planner.js";
import { packet } from "./fixtures.js";

const base = packet("J01");
const field = (name: string, artifact = "a1") => ({ name, artifact_id: artifact, pointer: `/${name}`, origin: "agent_generated" as const, coverage: [name] });
const shared = { disclosureScope: base.scope, allowedProviders: ["jev"], freshAfter: base.fresh_after, expiresAt: base.expires_at };
const q = (family: string, fields = [field("candidate"), field("evidence")], over: Partial<PlannedQuestion> = {}): PlannedQuestion =>
  ({ definition: definition(family), fields, dependsOn: [], ...shared, ...over });

it("puts independent families over the same evidence, disclosure and timing into one packet", () => {
  const waves = planPackets([q("J01"), q("J02"), q("J03")]);
  expect(waves).toHaveLength(1);
  expect(waves[0]).toHaveLength(1);
  expect(waves[0][0].questions.map((x) => x.definition.id)).toEqual(["J01", "J02", "J03"]);
  expect(waves[0][0].fields.map((f) => f.name)).toEqual(["candidate", "evidence"]);
});

it("separates questions whose evidence, disclosure or timing differ", () => {
  const waves = planPackets([
    q("J01"), q("J02", [field("claim", "a2"), field("evidence", "a2")]),
    q("J03", undefined, { allowedProviders: ["jev", "reference"] }),
    q("J04", undefined, { expiresAt: new Date(Date.parse(base.expires_at) + 1000).toISOString() }),
  ]);
  expect(waves).toHaveLength(1);
  expect(waves[0]).toHaveLength(4);
});

it("schedules a question depending on another family's answer in a later wave", () => {
  const waves = planPackets([q("J01"), q("J02"), q("J07", undefined, { dependsOn: ["J01"] })]);
  expect(waves).toHaveLength(2);
  expect(waves[0][0].questions.map((x) => x.definition.id)).toEqual(["J01", "J02"]);
  expect(waves[1][0].questions.map((x) => x.definition.id)).toEqual(["J07"]);
  expect(waves[1][0].questions[0].depends_on).toEqual(["J01"]);
});

it("rejects the same family twice in one plan and dependency cycles", () => {
  expect(() => planPackets([q("J01"), q("J01")])).toThrow(/J01/);
  expect(() => planPackets([q("J01", undefined, { dependsOn: ["J02"] }), q("J02", undefined, { dependsOn: ["J01"] })])).toThrow(/depend/i);
  expect(() => planPackets([q("J01", undefined, { dependsOn: ["J99"] })])).toThrow(/J99/);
});

it("splits a compatible group that exceeds the packet bound", () => {
  const families = ["J01", "J02", "J03", "J04", "J05", "J06", "J07", "J08", "J09", "J10"];
  const waves = planPackets(families.map((f) => q(f)), { maxQuestions: 4 });
  expect(waves).toHaveLength(1);
  expect(waves[0].map((p) => p.questions.length)).toEqual([4, 4, 2]);
});

it("preserves each question's evidence coverage when batching shared fields", () => {
  const first = { ...field("candidate"), coverage: ["assessment"] };
  const second = { ...field("candidate"), coverage: ["faithfulness"] };
  const waves = planPackets([q("J01", [first]), q("J02", [second])]);
  expect(waves.flat().flatMap((p) => p.fields.flatMap((f) => f.coverage))).toContain("faithfulness");
  expect(first.coverage).toEqual(["assessment"]);
});
