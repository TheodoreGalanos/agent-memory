import { describe, it, expect } from "vitest";
import capture from "../../../evals/formation/capture.json" with { type: "json" };
import type { FormationEntry } from "../../../contracts/generated/formation-window.js";
import { selected, families, evidencePointer } from "../src/index.js";
function entry(id: string): FormationEntry {
  const event = structuredClone(capture.events.find((e) => e.event_id === id)!);
  return { event, input: event.content } as unknown as FormationEntry;
}
describe("formation selection", () => {
  it("keeps temporary and hypothetical material out unless explicitly selected", () => {
    expect(selected(entry("scratch"))).toBe(false);
    expect(selected(entry("unselected-hypothesis"))).toBe(false);
    expect(selected(entry("selected-hypothesis"))).toBe(true);
    expect(selected(entry("instance-empty"))).toBe(true);
  });
  it("asks only the relevant independent families", () => {
    expect(families(entry("instance-empty"))).toEqual(["J01", "J02"]);
    expect(families(entry("type-property"))).toEqual(["J01", "J02", "J03"]);
    expect(families(entry("method"))).toEqual(["J01", "J02", "J04"]);
    expect(families(entry("preference"))).toEqual(["J01", "J02", "J27"]);
    expect(families(entry("intention"))).toEqual(["J01", "J02", "J05", "J27"]);
  });
  it("uses inspected window fields and rejects invented evidence fields", () => {
    expect(evidencePointer(2, "candidate")).toBe("/entries/2");
    expect(evidencePointer(2, "evidence")).toBe("/entries/2/input/evidence");
    expect(evidencePointer(2, "events")).toBe("/entries");
    expect(() => evidencePointer(2, "worker_claim")).toThrow();
  });
});
