// ABOUTME: Packet planner: groups independent questions that share evidence, disclosure and timing into one
// ABOUTME: packet and schedules dependent questions in a later wave, so a caller executes only what it needs.
import type { JudgementEvidence, JudgementDefinition, Scope } from "../../../contracts/generated/judgement-packet.js";
import type { PacketInput } from "./packet.js";

export interface PlannedQuestion {
  definition: JudgementDefinition;
  fields: Omit<JudgementEvidence, "content">[];
  /** Families whose answers this question needs; they run in an earlier wave. */
  dependsOn: string[];
  disclosureScope: Scope;
  allowedProviders: string[];
  freshAfter: string;
  expiresAt: string;
  localCheckId?: string;
}
/** A packet's questions, fields and shared terms, ready for `buildPacket` once given an id, subject and frame. */
export type PlannedPacket = Pick<PacketInput, "fields" | "questions" | "allowedProviders" | "freshAfter" | "expiresAt" | "localCheckId">;

const MAX_QUESTIONS = 32;

/** Returns waves of packets. Every packet in a wave is independent of the others in that wave; a packet in
 * wave n+1 depends on at least one family answered in wave n or earlier. */
export function planPackets(questions: PlannedQuestion[], options: { maxQuestions?: number } = {}): PlannedPacket[][] {
  const bound = Math.min(options.maxQuestions ?? MAX_QUESTIONS, MAX_QUESTIONS);
  if (bound < 1) throw new Error("A packet holds at least one question");
  const byFamily = new Map<string, PlannedQuestion>();
  for (const question of questions) {
    const id = question.definition.id;
    if (byFamily.has(id)) throw new Error(`Family ${id} is planned more than once`);
    byFamily.set(id, question);
  }
  for (const question of questions)
    for (const dependency of question.dependsOn)
      if (!byFamily.has(dependency)) throw new Error(`Family ${question.definition.id} depends on ${dependency}, which is not planned`);
  // Wave assignment by longest dependency chain; a cycle never resolves.
  const wave = new Map<string, number>();
  const depth = (id: string, trail: string[]): number => {
    const known = wave.get(id);
    if (known !== undefined) return known;
    if (trail.includes(id)) throw new Error(`Families ${[...trail, id].join(" -> ")} depend on each other`);
    const question = byFamily.get(id)!;
    const level = question.dependsOn.reduce((max, dependency) => Math.max(max, depth(dependency, [...trail, id]) + 1), 0);
    wave.set(id, level);
    return level;
  };
  for (const question of questions) depth(question.definition.id, []);
  const waves: PlannedPacket[][] = [];
  for (const question of questions) {
    const level = wave.get(question.definition.id)!;
    waves[level] ??= [];
    const compatible = waves[level].find((packet) => shares(packet, question) && packet.questions.length < bound);
    const entry = {
      definition: question.definition,
      disclosure_scope: question.disclosureScope,
      allowed_providers: question.allowedProviders,
      depends_on: question.dependsOn,
    };
    if (compatible) {
      compatible.questions.push(entry);
      continue;
    }
    waves[level].push({
      fields: question.fields.map((field) => ({ ...field })),
      questions: [entry],
      allowedProviders: [...question.allowedProviders],
      freshAfter: question.freshAfter,
      expiresAt: question.expiresAt,
      localCheckId: question.localCheckId,
    });
  }
  return waves;
}

/** Same evidence fields, disclosure grant, freshness, expiry and local check: one shared state. */
function shares(packet: PlannedPacket, question: PlannedQuestion): boolean {
  const same = (a: Omit<JudgementEvidence, "content">, b: Omit<JudgementEvidence, "content">) =>
    a.name === b.name && a.artifact_id === b.artifact_id && a.pointer === b.pointer && a.origin === b.origin;
  return (
    packet.fields.length === question.fields.length &&
    packet.fields.every((field, index) => same(field, question.fields[index])) &&
    packet.allowedProviders.join("\0") === question.allowedProviders.join("\0") &&
    JSON.stringify(packet.questions[0].disclosure_scope) === JSON.stringify(question.disclosureScope) &&
    packet.freshAfter === question.freshAfter &&
    packet.expiresAt === question.expiresAt &&
    packet.localCheckId === question.localCheckId
  );
}
