import type { JudgementPacket } from "../../../contracts/generated/judgement-packet.js";
import type {
  SemanticAssessment,
  JudgementAnswer,
  JudgementUsage,
} from "../../../contracts/generated/semantic-assessment.js";

export const tolerance = 0.001;
// Allow rounded probability totals without changing choices, scores or confidence.
export const distributionSumTolerance = 0.01;
export function object(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error("Expected an object");
  return value as Record<string, unknown>;
}
function probability(value: unknown): value is number {
  return (
    typeof value === "number" &&
    Number.isFinite(value) &&
    value >= 0 &&
    value <= 1
  );
}
function sameKeys(value: Record<string, unknown>, keys: string[]) {
  return Object.keys(value).sort().join("\0") === [...keys].sort().join("\0");
}
export function questionMap(packet: JudgementPacket) {
  return Object.fromEntries(
    packet.questions.flatMap((item) =>
      item.definition.questions.map((q) => [
        `${item.definition.id}.${q.key}`,
        { ...q.form, instructions: q.instructions },
      ]),
    ),
  );
}
export function usageFrom(raw: unknown): JudgementUsage {
  try {
    const usage = object(object(raw).usage);
    const count = (v: unknown) =>
      typeof v === "number" &&
      Number.isSafeInteger(v) &&
      v >= 0 &&
      v <= 4294967295
        ? v
        : null;
    return {
      input_tokens: count(usage.input_tokens),
      output_tokens: count(usage.output_tokens),
      cost_microunits: null,
    };
  } catch {
    return { input_tokens: null, output_tokens: null, cost_microunits: null };
  }
}
export function validateResponse(
  packet: JudgementPacket,
  raw: unknown,
  distributionRequired: boolean,
): Pick<SemanticAssessment, "answers" | "returned_model"> {
  const body = object(raw);
  if (typeof body.model !== "string" || !body.model.trim())
    throw new Error("Response has no returned model identity");
  const answers = object(body.answers),
    questions = questionMap(packet);
  if (!sameKeys(answers, Object.keys(questions)))
    throw new Error("Answer keys do not match the packet");
  const normalized: Record<string, JudgementAnswer> = {};
  for (const [key, question] of Object.entries(questions)) {
    const answer = object(answers[key]);
    if (answer.type !== question.type)
      throw new Error(`Wrong answer type for ${key}`);
    if (question.type === "noul") {
      if (
        !probability(answer.noul) ||
        "confidence" in answer ||
        "probabilities" in answer
      )
        throw new Error(
          "Noul is a proposition probability, not a separate confidence",
        );
      normalized[key] = { type: "noul", noul: answer.noul };
      continue;
    }
    const keys =
      question.type === "choice"
        ? Object.keys(question.criteria)
        : question.criteria.map((_, i) => String(i));
    let probabilities: Record<string, number> | undefined;
    let confidence: number | undefined;
    if (
      distributionRequired ||
      answer.probabilities !== undefined ||
      answer.confidence !== undefined
    ) {
      const values = object(answer.probabilities);
      if (
        !sameKeys(values, keys) ||
        !Object.values(values).every(probability) ||
        !probability(answer.confidence)
      )
        throw new Error("Invalid distribution fields");
      probabilities = values as Record<string, number>;
      if (
        Math.abs(Object.values(probabilities).reduce((a, b) => a + b, 0) - 1) >
        distributionSumTolerance + Number.EPSILON * keys.length
      )
        throw new Error("Distribution is not normalised");
      confidence = answer.confidence;
    }
    if (question.type === "choice") {
      if (
        typeof answer.choice !== "string" ||
        !keys.includes(answer.choice) ||
        (probabilities &&
          Object.values(probabilities).some(
            (v) => v > probabilities![answer.choice as string] + tolerance,
          ))
      )
        throw new Error(
          "Choice is not an allowed highest-probability alternative",
        );
      normalized[key] = {
        type: "choice",
        choice: answer.choice,
        probabilities,
        confidence,
      };
    } else {
      const legend = object(answer.legend);
      if (
        !sameKeys(legend, keys) ||
        question.criteria.some((v, i) => legend[String(i)] !== v) ||
        typeof answer.score !== "number" ||
        !Number.isFinite(answer.score) ||
        answer.score < 0 ||
        answer.score > keys.length - 1
      )
        throw new Error("Score or legend differs from the rubric");
      if (
        probabilities &&
        Math.abs(
          Object.entries(probabilities).reduce(
            (sum, [level, p]) => sum + Number(level) * p,
            0,
          ) - answer.score,
        ) > tolerance
      )
        throw new Error("Score differs from its probability-weighted value");
      normalized[key] = {
        type: "score",
        score: answer.score,
        legend: legend as Record<string, string>,
        probabilities,
        confidence,
      };
    }
  }
  return { answers: normalized, returned_model: body.model };
}
