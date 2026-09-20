// ABOUTME: Explicit response rules for the catalogue families: for each question, which non-structural choices
// ABOUTME: call for which action. Structural outcomes (insufficient or missing evidence) belong to the runtime.
import type { PolicyAction } from "../../../contracts/generated/judgement-decision.js";
import { familyResponse, type ResponseRules } from "./policy.js";

/** Choice → action per question key. Choices not listed leave the assessment for the process to qualify. */
export const catalogueResponseTable: Record<string, Record<string, Record<string, PolicyAction>>> = {
  J01: {
    assessment: { mixed: "investigate", other: "investigate" },
    faithfulness: { no: "revise" }
  },
  J02: {
    assessment: { contradicts: "investigate", both: "investigate", unresolved: "investigate", not_addressed: "retrieve_further", other: "investigate" }
  },
  J03: {
    assessment: { other: "investigate" }
  },
  J04: {
    assessment: { untested_generalisation: "qualify", mixed: "investigate", other: "investigate" }
  },
  J05: {
    assessment: { mixed: "investigate", other: "investigate" }
  },
  J06: {
    direct: {},
    background: {},
    exception: {},
    contradiction: { yes: "investigate" }
  },
  J07: {
    assessment: { other: "investigate" }
  },
  J08: {
    assessment: { unmet: "defer", other: "investigate" }
  },
  J09: {
    assessment: { different: "qualify", other: "investigate" }
  },
  J10: {
    assessment: { different: "qualify", other: "investigate" }
  },
  J11: {
    comparable: {},
    counterexample: { yes: "investigate" },
    duplicate: { yes: "investigate" }
  },
  J12: {
    conditions: { no: "revise" },
    uncertainty: { no: "revise" },
    scope: { no: "revise" }
  },
  J13: {
    assessment: { unresolved: "investigate", other: "investigate" }
  },
  J14: {
    assessment: { incompatible: "investigate", other: "investigate" }
  },
  J15: {
    assessment: { partial: "qualify", none: "retrieve_further", other: "investigate" }
  },
  J16: {
    assessment: { affected: "investigate", other: "investigate" }
  },
  J17: {
    assessment: { not_satisfied: "defer", other: "investigate" }
  },
  J18: {
    definitions: { no: "retrieve_further" },
    versions: { no: "retrieve_further" },
    scope: { no: "retrieve_further" },
    outputs: { no: "retrieve_further" },
    evidence: { no: "retrieve_further" }
  },
  J19: {
    conditions: { no: "revise" },
    uncertainty: { no: "revise" },
    conflicts: { no: "revise" },
    obligations: { no: "revise" },
    checks: { no: "revise" }
  },
  J20: {
    duplicate: {},
    conflicting: { yes: "investigate" },
    shared_support: { yes: "investigate" }
  },
  J21: {
    assessment: { partial: "qualify", overclaimed: "revise", other: "investigate" }
  },
  J22: {
    scope: { no: "revise" },
    findings: { no: "revise" },
    status: { no: "revise" }
  },
  J23: {
    assessment: { suspicious: "investigate", other: "investigate" }
  },
  J24: {
    assessment: { unchanged_failure: "revise", other: "investigate" }
  },
  J25: {
    assessment: { other: "investigate" }
  },
  J26: {
    assessment: { challenged: "investigate", mixed: "investigate", unknown: "retrieve_further", other: "investigate" }
  },
  J27: {
    assessment: { mixed: "investigate", other: "investigate" }
  },
  J28: {
    assessment: { requires_intent: "request_input", evidence_can_resolve: "retrieve_further", other: "investigate" }
  },
  J30: {
    role: { other: "investigate" }
  },
  J29: {
    interpretation: { yes: "investigate" },
    evidence: { yes: "investigate" },
    check: { yes: "investigate" }
  },
  J31: {
    next: { other: "investigate" }
  },
};

/** The rules the catalogue owners declare, ready to pass as `responses` to the runtime. */
export const catalogueResponses: ResponseRules = Object.fromEntries(
  Object.entries(catalogueResponseTable).map(([family, questions]) => [family, familyResponse(questions)]),
);
