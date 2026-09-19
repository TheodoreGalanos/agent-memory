// ABOUTME: Tests for constructing candidates from experience spans: content is copied from the span's own
// ABOUTME: material with the span's attribution, per role; spans without copyable material yield no candidate.
import { expect, it } from "vitest";
import type { ExperienceSpan, ExperienceWindow } from "../../../contracts/generated/host-response.js";
import { constructCandidate } from "../src/candidates.js";

const context = { objective: "Establish the clearance under beam B3.", result_status: "complete" };
const at = new Date().toISOString();
const exchange: ExperienceSpan = {
  index: 0, start: 2, end: 3, kind: "exchange",
  events: [
    { event_id: "2-tool_call", observed_at: at, kind: "tool_call", origin: "agent_generated", evidential_status: "inference", content: { tool: "read", call_id: "c1", arguments: { path: "notes.txt" } } },
    { event_id: "3-tool_result", observed_at: at, kind: "tool_result", origin: "observed", evidential_status: "observation", content: { tool: "read", call_id: "c1", is_error: false, text: "Clearance under B3 measured at 2.0 m.", truncated: false } },
  ],
};
const claimText = "Always check the site notes before trusting a drawing's clearance.";
const claim: ExperienceSpan = {
  index: 1, start: 4, end: 4, kind: "message",
  events: [{ event_id: "4-message", observed_at: at, kind: "message", origin: "agent_generated", evidential_status: "inference", content: { role: "assistant", text: claimText, truncated: false } }],
};
const window = { source: { source_id: "s", revision: "1" }, context } as unknown as ExperienceWindow;

it("copies a tool result into a knowledge candidate attributed as an observation", () => {
  const proposal = constructCandidate(window, exchange, "knowledge", "d1")!;
  expect(proposal).toMatchObject({ span: 0, decision: "d1", origin: "observed", evidential_status: "observation" });
  expect(proposal.input.content).toMatchObject({ family: "knowledge", statement: "Clearance under B3 measured at 2.0 m." });
  expect(proposal.input.objective).toBe(context.objective);
  expect(proposal.input.boundary_explicit).toBe(true);
  expect(proposal.input.explicitly_selected).toBe(false);
  expect(proposal.input.evidence).toEqual(exchange.events);
  expect(proposal.input.coverage.examined).toEqual(["span-0"]);
});

it("builds an episode from what was done and observed, and a procedure from an assistant's method", () => {
  const episode = constructCandidate(window, exchange, "episode", "d2")!;
  expect(episode.input.content).toMatchObject({ family: "episode", actions: ["read"], observations: ["Clearance under B3 measured at 2.0 m."], outcome: "Clearance under B3 measured at 2.0 m." });
  const procedure = constructCandidate(window, claim, "procedure", "d3")!;
  expect(procedure).toMatchObject({ origin: "agent_generated", evidential_status: "inference" });
  expect(procedure.input.content).toMatchObject({ family: "procedure", purpose: claimText, method: { form: "advisory", steps: [claimText] } });
  expect(procedure.input.boundary_explicit).toBe(false);
});

it("yields no candidate when the span has no copyable text or an intention has no owner", () => {
  const empty: ExperienceSpan = { index: 2, start: 5, end: 5, kind: "result", events: [{ event_id: "5-result", observed_at: at, kind: "result", origin: "agent_generated", evidential_status: "inference", content: { result: { status: "complete" } } }] };
  expect(constructCandidate(window, empty, "knowledge", "d4")).toBeNull();
  expect(constructCandidate(window, claim, "intention", "d5")).toBeNull();
  const owned = { ...window, scope: { user_id: "11111111-1111-1111-1111-111111111111" } } as unknown as ExperienceWindow;
  expect(constructCandidate(owned, claim, "intention", "d5")!.input.content).toMatchObject({ family: "intention", owner_id: "11111111-1111-1111-1111-111111111111", trigger: claimText });
});
