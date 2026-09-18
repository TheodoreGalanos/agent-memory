import { randomUUID } from "node:crypto";
import { BACKGROUND_CONTEXT as context } from "@earendil-works/pi-agent-core";
import { it, expect, vi } from "vitest";
import type { Job } from "../../../contracts/generated/assignment.js";
import { inspectJson, preserveChildEvidence } from "../src/scoped-operation.js";
import { input, scriptedResult } from "./fixtures.js";

function fixture() {
  const brief = input("before-correction").command.payload;
  brief.limits.max_output_bytes = 65536;
  const result = scriptedResult("before-correction");
  const child: Job = {
    id: randomUUID(),
    root_id: randomUUID(),
    operation_id: randomUUID(),
    session_id: randomUUID(),
    depth: 1,
    attempt: 1,
    state: "completed",
    deadline: new Date(Date.now() + 60_000).toISOString(),
    cancel_requested: false,
    spec: {
      brief,
      max_attempts: 3,
      retain_until: new Date(Date.now() + 60_000).toISOString(),
    },
    result: structuredClone(result),
  };
  child.result!.result_artifact = randomUUID();
  return { brief, result, child };
}

it("expands JSON definitions and reports missing or malformed paths", async () => {
  const host = {
    request: vi.fn(async () => ({
      kind: "artifact_data" as const,
      text: '{"a/b":{"~name":["clearance"]}}',
    })),
  };
  const fence = { job_id: randomUUID(), owner_id: randomUUID(), epoch: 1 };
  expect(
    await inspectJson(host, fence, randomUUID(), "/a~1b/~0name/0", context),
  ).toBe("clearance");
  await expect(
    inspectJson(host, fence, randomUUID(), "/missing", context),
  ).rejects.toThrow("Required definition is missing");
  await expect(
    inspectJson(host, fence, randomUUID(), "/~2", context),
  ).rejects.toThrow("Invalid JSON pointer");
});

it("missing child output remains unknown and is never treated as a negative finding", () => {
  const { brief, result, child } = fixture();
  child.result = null;
  child.state = "failed";
  const output = preserveChildEvidence(result, [child], randomUUID(), brief);
  expect(output.status).toBe("partial");
  expect(output.coverage.unexamined).toContain(child.spec.brief.purpose);
  expect(output.findings).toEqual(result.findings);
  expect(output.unresolved_work.join(" ")).toContain(
    "without validated output",
  );
});

it("retains opposing findings and does not widen their applicability", () => {
  const { brief, result, child } = fixture();
  child.result!.findings[0].statement =
    "Counterexample: one entity fails the check";
  child.result!.findings[0].applicability.entity_ids = [randomUUID()];
  const output = preserveChildEvidence(result, [child], randomUUID(), brief);
  expect(output.findings).toContainEqual(child.result!.findings[0]);
  expect(output.findings.at(-1)?.applicability).toEqual(
    child.result!.findings[0].applicability,
  );
  const widened = structuredClone(result);
  widened.examined_scope.project_id = null;
  expect(() =>
    preserveChildEvidence(widened, [child], randomUUID(), brief),
  ).toThrow("exceeded");
});

it("publishes an explicitly partial reference result when child findings exceed the bound", () => {
  const { brief, result, child } = fixture();
  brief.limits.max_output_bytes = 5000;
  child.result!.findings[0].statement = "Counterexample ".repeat(1000);
  const basisId = randomUUID();
  const output = preserveChildEvidence(result, [child], basisId, brief);
  expect(Buffer.byteLength(JSON.stringify(output)) + 512).toBeLessThanOrEqual(
    5000,
  );
  expect(output.status).toBe("partial");
  expect(output.child_outputs).toContain(child.result!.result_artifact);
  expect(output.child_outputs).toContain(basisId);
  expect(output.coverage.unexamined.join(" ")).toContain(
    "inspect the referenced child results",
  );
  expect(child.result!.findings[0].statement).toContain("Counterexample");
});
