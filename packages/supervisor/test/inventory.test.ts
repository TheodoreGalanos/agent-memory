// ABOUTME: Asserts the capability inventory script runs and lists every pool module and judgement family.
import { expect, it } from "vitest";
import { inventory } from "../../../scripts/inventory.mjs";

it("lists all seven pool modules and 31 judgement catalogue families", () => {
  const { markdown, modules, families } = inventory();
  expect(modules.map((m) => m.id).sort()).toEqual(
    ["activation", "consolidation", "decision", "evaluation", "formation", "investigation", "maintenance"].sort(),
  );
  expect(families).toHaveLength(31);
  expect(markdown).toContain("## Pool modules (7)");
  expect(markdown).toContain("## Judgement catalogue families (31)");
  expect(markdown).toContain("| decision | 1 |");
  expect(markdown).toContain("| J01 | Formation |");
});
