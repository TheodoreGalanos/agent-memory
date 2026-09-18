import type {
  JudgementPacket,
  Scope,
} from "../../../contracts/generated/judgement-packet.js";
import type { SemanticAssessment } from "../../../contracts/generated/semantic-assessment.js";
import type {
  JudgementDecision,
  PolicyAction,
} from "../../../contracts/generated/judgement-decision.js";
import { permits } from "../../pi-worker/src/workspace.js";
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
export function qualified(
  packet: JudgementPacket,
  assessment: SemanticAssessment,
  qualifications: Qualification[],
): boolean {
  return (
    assessment.status === "answered" &&
    packet.questions.every(({ definition }) => {
      const gate = qualifications.find(
        (q) =>
          q.family === definition.id &&
          q.revision === definition.revision &&
          q.modelRelease === assessment.model_release &&
          permits(q.scope, packet.scope),
      );
      if (
        !gate ||
        !gate.evaluationArtifact ||
        !(gate.minimumProbability > 0 && gate.minimumProbability <= 1)
      )
        return false;
      return definition.questions.every((q) => {
        const a = assessment.answers[`${definition.id}.${q.key}`];
        // Choice admission uses the probability of the selected option, never the
        // provider's distribution-sharpness confidence. Other forms need their own policy.
        return (
          a?.type === "choice" &&
          !["insufficient_evidence", "other"].includes(a.choice) &&
          (a.probabilities?.[a.choice] ?? -1) >= gate.minimumProbability
        );
      });
    })
  );
}
const concern: Record<string, PolicyAction> = {
  contradicts: "investigate",
  both: "investigate",
  affected: "investigate",
  incompatible: "investigate",
  inconsistent: "revise",
  suspicious: "investigate",
  challenged: "investigate",
  mixed: "investigate",
  unresolved: "investigate",
  unknown: "retrieve_further",
  overclaimed: "revise",
  partial: "qualify",
  none: "retrieve_further",
  not_addressed: "retrieve_further",
  unmet: "defer",
  not_satisfied: "defer",
  unchanged_failure: "revise",
  requires_intent: "request_input",
  evidence_can_resolve: "retrieve_further",
  untested_generalisation: "qualify",
  different: "qualify",
  other: "investigate",
};
const negativeMatters = new Set([
  "J01.faithfulness",
  "J12.conditions",
  "J12.uncertainty",
  "J12.scope",
  "J18.definitions",
  "J18.versions",
  "J18.scope",
  "J18.outputs",
  "J18.evidence",
  "J19.conditions",
  "J19.uncertainty",
  "J19.conflicts",
  "J19.obligations",
  "J19.checks",
  "J22.scope",
  "J22.findings",
  "J22.status",
]);
const positiveMatters = new Set([
  "J06.contradiction",
  "J11.counterexample",
  "J11.duplicate",
  "J20.conflicting",
  "J20.shared_support",
  "J29.interpretation",
  "J29.evidence",
  "J29.check",
]);
const priority: PolicyAction[] = [
  "defer",
  "request_input",
  "retrieve_further",
  "revise",
  "investigate",
  "qualify",
  "retain",
];
export function decide(
  packet: JudgementPacket,
  id: string,
  assessments: SemanticAssessment[],
  selected: SemanticAssessment | undefined,
  mode: string,
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
      const answers = d.questions.map(
        (q) => [q.key, selected?.answers[`${d.id}.${q.key}`]] as const,
      );
      const insufficient = answers.some(
        ([, a]) => a?.type === "choice" && a.choice === "insufficient_evidence",
      );
      const actions: PolicyAction[] = [];
      for (const [key, a] of answers) {
        if (a?.type !== "choice") {
          actions.push("investigate");
          continue;
        }
        if (negativeMatters.has(`${d.id}.${key}`) && a.choice === "no")
          actions.push(d.id === "J18" ? "retrieve_further" : "revise");
        if (positiveMatters.has(`${d.id}.${key}`) && a.choice === "yes")
          actions.push("investigate");
        if (concern[a.choice]) actions.push(concern[a.choice]);
      }
      const action: PolicyAction = !selected
        ? "defer"
        : missing.length || insufficient
          ? "retrieve_further"
          : packet.local_check_id
            ? "investigate"
            : (priority.find((a) => actions.includes(a)) ?? "qualify");
      const findings = answers
        .map(
          ([key, a]) =>
            `${key}=${a?.type === "choice" ? a.choice : a?.type === "noul" ? a.noul : a?.type === "score" ? a.score : "unavailable"}`,
        )
        .join("; ");
      return [
        d.id,
        {
          policy: packet.policy,
          action,
          reason: `${findings}. ${catalogue.find((x) => x.id === d.id)?.behaviour ?? d.fallback}`,
          constraints: [
            "Assessment only; the owning process must validate any proposed effect.",
            "Required deterministic checks, receipts and authority remain mandatory.",
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
