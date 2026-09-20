// ABOUTME: Constructs a formation candidate from one experience span and the role judgement assigned it. Text is
// ABOUTME: copied from the span's own material and attributed as the span attributes it; nothing is invented.
import type { ExperienceSpan, ExperienceWindow, ToolEvent } from "../../../contracts/generated/host-response.js";
import type { CandidateProposal } from "../../../contracts/generated/host-request.js";
import type { FormationInput } from "../../../contracts/generated/formation-window.js";

export type CandidateRole = "episode" | "knowledge" | "procedure" | "intention";
export const CANDIDATE_ROLES: readonly CandidateRole[] = ["episode", "knowledge", "procedure", "intention"];

const text = (event: ToolEvent | undefined) => {
  const value = (event?.content as { text?: unknown } | undefined)?.text;
  return typeof value === "string" && value.trim() ? value : undefined;
};

/** The event whose text a candidate copies: a tool's result over a call, a message's own text. */
function material(span: ExperienceSpan): { event: ToolEvent; text: string } | undefined {
  const preferred = [...span.events].reverse().find((e) => e.kind === "tool_result" && text(e)) ?? span.events.find((e) => text(e));
  const copied = text(preferred);
  return preferred && copied ? { event: preferred, text: copied } : undefined;
}

/** Returns null when the span holds nothing that can be copied for the role; the caller reports that. */
export function constructCandidate(
  window: ExperienceWindow,
  span: ExperienceSpan,
  role: CandidateRole,
  decision: string,
): CandidateProposal | null {
  const source = material(span);
  if (!source) return null;
  const call = span.events.find((e) => e.kind === "tool_call");
  const tool = (call?.content as { tool?: unknown } | undefined)?.tool;
  const content = ((): FormationInput["content"] | null => {
    switch (role) {
      case "knowledge":
        return { family: "knowledge", statement: source.text, subject: null, predicate: null, uncertainty: [], examined_coverage: [] };
      case "episode":
        return {
          family: "episode",
          objective: window.context.objective,
          initial_conditions: [],
          observations: span.events.filter((e) => e.kind === "tool_result" && text(e)).map((e) => text(e)!),
          actions: typeof tool === "string" ? [tool] : [],
          corrections: [],
          outcome: source.text,
          verification: [],
          uncertainty: [],
        };
      case "procedure":
        return {
          family: "procedure",
          purpose: source.text,
          capabilities: [],
          applicability: [],
          exclusions: [],
          method: { form: "advisory", steps: [source.text], evidence_criteria: [] },
          counterexamples: [],
        };
      case "intention": {
        const owner = window.scope?.user_id;
        if (!owner) return null;
        return {
          family: "intention",
          purpose: source.text,
          owner_id: owner,
          trigger: source.text,
          readiness: [],
          completion: [],
          expires_at: null,
          notification_policy: "material",
          recurrence: null,
        };
      }
    }
  })();
  if (!content) return null;
  const input: FormationInput = {
    episode: `experience ${window.source.source_id.slice(0, 8)}`,
    objective: window.context.objective,
    conditions: [`span ${span.index} of ${window.source.source_id}@${window.source.revision}`],
    // A tool call with its result is a bounded event by construction; a message's boundary is judged.
    boundary_explicit: span.kind === "exchange",
    retention: "optional",
    explicitly_selected: false,
    explicit_contribution: false,
    actor_id: null,
    content,
    evidence: span.events as unknown as FormationInput["evidence"],
    source_locators: [],
    corrects: [],
    based_on: [],
    uncertainty: [],
    coverage: { examined: [`span-${span.index}`], unexamined: [] },
  };
  return { span: span.index, decision, origin: source.event.origin, evidential_status: source.event.evidential_status, input };
}
