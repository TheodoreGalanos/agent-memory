// ABOUTME: Asserts the campaign scaffold's dry run drives all three arms and every fixture task to a
// ABOUTME: settled, adapter-checked outcome, entirely without live provider credentials.
import { expect, it } from "vitest";
import { dryRun, ARMS } from "../../../evals/campaign/run.js";

it("runs the 3 fixture tasks through every arm without any live provider, settling every outcome", async () => {
  const report = await dryRun();
  expect(report.arms).toEqual(ARMS);
  expect(report.outcomes).toHaveLength(3 * ARMS.length);
  expect(report.outcomes.every((o) => o.operational === "settled"), JSON.stringify(report.outcomes)).toBe(true);
  // The transfer tasks show memory's behavioural value: the full arm costs no more, and for the task
  // that only the full arm can actually answer, it is also the only arm that passes semantically.
  expect(report.behavioural_value).toHaveLength(2);
  const harder = report.behavioural_value.find((b) => b.task_id === "lease-timeout-default")!;
  expect(harder.full_cheaper_than_no_memory).toBe(true);
  expect(harder.full_more_complete).toBe(true);
  expect(report.cost_by_arm.full).toBeLessThan(report.cost_by_arm.no_memory);
}, 30_000);
