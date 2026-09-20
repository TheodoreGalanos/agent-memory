// ABOUTME: Judgement policy: form-aware admission, evaluation status, process-owned response rules and the
// ABOUTME: decision assembly that keeps structural outcomes distinct from label interpretation.
import type {
  JudgementPacket,
  Scope,
} from "../../../contracts/generated/judgement-packet.js";
import type {
  JudgementAnswer,
  SemanticAssessment,
} from "../../../contracts/generated/semantic-assessment.js";
import type { JudgementDefinition } from "../../../contracts/generated/judgement-packet.js";
import type {
  JudgementDecision,
  PolicyAction,
} from "../../../contracts/generated/judgement-decision.js";
import { permits } from "../../host-client/src/scope.js";
import { catalogue } from "./catalogue.js";

export interface Qualification {
  family: string;
  revision: number;
  modelRelease: string;
  scope: Scope;
  evaluationArtifact: string;
  /** Chosen from held-out results for this definition, release and domain. */
  minimumProbability: number;
}
/** Whether an answer asserts its proposition with at least `minimumProbability`, in the terms of its own
 * form: the selected choice's probability, the Noul probability (either way), or the chosen score level's
 * probability. Confidence is distribution sharpness and never substitutes for these. */
export function admissible(
  answer: JudgementAnswer | undefined,
  minimumProbability: number,
): boolean {
  if (!answer) return false;
  if (answer.type === "noul")
    return Math.max(answer.noul, 1 - answer.noul) >= minimumProbability;
  if (answer.type === "choice")
    return (
      !["insufficient_evidence", "other"].includes(answer.choice) &&
      (answer.probabilities?.[answer.choice] ?? -1) >= minimumProbability
    );
  return (
    Number.isInteger(answer.score) &&
    (answer.probabilities?.[String(answer.score)] ?? -1) >= minimumProbability
  );
}
export type EvaluationStatus = "qualified" | "experimental";
/** A family is qualified for this assessment when a held-out gate covers its definition revision, the
 * assessed model release and the packet scope, and every answer meets the gate's probability. */
export function evaluationStatus(
  packet: JudgementPacket,
  definition: JudgementDefinition,
  assessment: SemanticAssessment,
  qualifications: Qualification[],
): EvaluationStatus {
  const gate = qualifications.find(
    (q) =>
      q.family === definition.id &&
      q.revision === definition.revision &&
      q.modelRelease === assessment.model_release &&
      permits(q.scope, packet.scope),
  );
  const usable =
    assessment.status === "answered" &&
    !!gate?.evaluationArtifact &&
    gate.minimumProbability > 0 &&
    gate.minimumProbability <= 1 &&
    definition.questions.every((q) =>
      admissible(
        assessment.answers[`${definition.id}.${q.key}`],
        gate.minimumProbability,
      ),
    );
  return usable ? "qualified" : "experimental";
}
export function qualified(
  packet: JudgementPacket,
  assessment: SemanticAssessment,
  qualifications: Qualification[],
): boolean {
  return packet.questions.every(
    ({ definition }) =>
      evaluationStatus(packet, definition, assessment, qualifications) ===
      "qualified",
  );
}

/** A process-owned rule mapping one family's answers (keyed by question key) to the action that
 * process permits. The runtime interprets no labels itself. */
export type ResponseRule = (
  answers: Record<string, JudgementAnswer | undefined>,
  definition: JudgementDefinition,
) => PolicyAction;
export type ResponseRules = Record<string, ResponseRule>;
export function choiceResponse(
  key: string,
  actions: Record<string, PolicyAction>,
  otherwise: PolicyAction,
): ResponseRule {
  return (answers) => {
    const a = answers[key];
    return a?.type === "choice" ? (actions[a.choice] ?? otherwise) : otherwise;
  };
}
export function noulResponse(
  key: string,
  rule: { atLeast: number; then: PolicyAction; otherwise: PolicyAction },
): ResponseRule {
  return (answers) => {
    const a = answers[key];
    return a?.type === "noul" && a.noul >= rule.atLeast ? rule.then : rule.otherwise;
  };
}
/** Actions by rubric level; a fractional score rounds to its nearest level. */
export function scoreResponse(
  key: string,
  byLevel: PolicyAction[],
  otherwise: PolicyAction,
): ResponseRule {
  return (answers) => {
    const a = answers[key];
    return a?.type === "score"
      ? (byLevel[Math.round(a.score)] ?? otherwise)
      : otherwise;
  };
}

/** From most to least conservative: when a family's questions call for different actions, the earlier wins. */
export const conservatism: PolicyAction[] = [
  "defer",
  "request_input",
  "retrieve_further",
  "revise",
  "investigate",
  "qualify",
  "retain",
];
/** One family's rule from per-question choice maps; a family with no matching choice is left to qualify. */
export function familyResponse(
  questions: Record<string, Record<string, PolicyAction>>,
): ResponseRule {
  return (answers) => {
    const called = Object.entries(questions).flatMap(([key, actions]) => {
      const a = answers[key];
      return a?.type === "choice" && actions[a.choice] ? [actions[a.choice]] : [];
    });
    return conservatism.find((action) => called.includes(action)) ?? "qualify";
  };
}

const render = (a: JudgementAnswer | undefined) =>
  a?.type === "choice" ? a.choice : a?.type === "noul" ? a.noul : a?.type === "score" ? a.score : "unavailable";

/** Structural outcomes come first and stay distinct: no selected assessment, missing or insufficient
 * evidence, and task-local checks. Only then does the owning process's rule speak; without one the
 * assessment is left for that process to qualify. Evaluation status is reported, never gating. */
export function decide(
  packet: JudgementPacket,
  id: string,
  assessments: SemanticAssessment[],
  selected: SemanticAssessment | undefined,
  mode: string,
  responses: ResponseRules = {},
  qualifications: Qualification[] = [],
): JudgementDecision {
  const inconsistencies: string[] = [];
  if (assessments.filter((a) => a.status === "answered").length > 1) {
    for (const q of packet.questions.flatMap((q) =>
      q.definition.questions.map((x) => `${q.definition.id}.${x.key}`),
    )) {
      const choices = new Set(
        assessments.flatMap((a) =>
          a.answers[q]?.type === "choice" ? [a.answers[q].choice] : [],
        ),
      );
      if (choices.size > 1)
        inconsistencies.push(
          `Provider disagreement at ${q}: ${[...choices].join(", ")}`,
        );
    }
  }
  const decisions = Object.fromEntries(
    packet.questions.map(({ definition: d }) => {
      const missing = d.input_requirements.filter((r) =>
        packet.missing.includes(r),
      );
      const answers = Object.fromEntries(
        d.questions.map((q) => [q.key, selected?.answers[`${d.id}.${q.key}`]]),
      );
      const insufficient = Object.values(answers).some(
        (a) => a?.type === "choice" && a.choice === "insufficient_evidence",
      );
      const action: PolicyAction = !selected
        ? "defer"
        : missing.length || insufficient
          ? "retrieve_further"
          : packet.local_check_id
            ? "investigate"
            : (responses[d.id]?.(answers, d) ?? "qualify");
      const findings = Object.entries(answers)
        .map(([key, a]) => `${key}=${render(a)}`)
        .join("; ");
      const status = selected
        ? evaluationStatus(packet, d, selected, qualifications)
        : undefined;
      return [
        d.id,
        {
          policy: packet.policy,
          action,
          reason: `${findings}. ${catalogue.find((x) => x.id === d.id)?.behaviour ?? d.fallback}`,
          constraints: [
            "Assessment only; the owning process must validate any proposed effect.",
            "Required deterministic checks, receipts and authority remain mandatory.",
            ...(status
              ? [
                  `Evaluation status: ${status}${status === "experimental" ? ` (no held-out qualification covers ${d.id} r${d.revision} for ${selected?.model_release ?? selected?.requested_model})` : ""}.`,
                ]
              : []),
          ],
          required_evidence: missing.length
            ? missing
            : insufficient
              ? d.input_requirements
              : [],
          expires_at: packet.expires_at,
        },
      ];
    }),
  );
  return {
    id,
    packet_id: packet.id,
    assessment_ids: assessments.map((a) => a.id),
    selected_assessment: selected?.id,
    mode,
    inconsistencies,
    decisions,
  };
}

/** Supplied by the owning policy only where questions refer to the same subject. */
export interface ConsistencyRule {
  id: string;
  allOf: { question: string; choice: string }[];
}
export function inconsistencies(
  assessment: SemanticAssessment,
  rules: ConsistencyRule[],
): string[] {
  return rules
    .filter((rule) =>
      rule.allOf.every((condition) => {
        const answer = assessment.answers[condition.question];
        return answer?.type === "choice" && answer.choice === condition.choice;
      }),
    )
    .map((rule) => rule.id);
}
