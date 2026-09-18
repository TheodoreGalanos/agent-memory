import { expect, it } from "vitest";
import fixture from "../../../evals/property-location/inputs/before-correction.json" with { type: "json" };
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type { HostCommands } from "../../pi-worker/src/host-client.js";
import type { HostResponse } from "../../../contracts/generated/host-response.js";
import { workspaceFromBrief } from "../../pi-worker/src/workspace.js";
import { reconcileActiveWork } from "../src/workspace.js";
it("follows a correction after a wording revision without rewriting the original citation", async () => {
  const assignment = {
    job: { spec: { brief: fixture.command.payload } },
    owner_id: "worker",
    epoch: 1,
  } as unknown as Assignment;
  const state = workspaceFromBrief(
    assignment.job.spec.brief,
    "2099-01-01T00:00:00Z",
    "2099-02-01T00:00:00Z",
  );
  const ref = { memory_id: "memory", revision: 1, label: "Original wording" };
  state.entries[0].inputs.memories = [ref];
  const host: HostCommands = {
    async request() {
      return {
        kind: "changes",
        page: {
          cursor: 3,
          snapshot_required: false,
          changes: [
            {
              id: "notice",
              cursor: 3,
              kind: "correction",
              previous: { ...ref, revision: 2 },
              current: { ...ref, revision: 3 },
              scope: state.scope,
              valid_time: { kind: "unknown" },
              reason: "Corrected finding",
            },
          ],
        },
      } as HostResponse;
    },
  };
  const result = await reconcileActiveWork(host, assignment, state, 1);
  expect(result.workspace.entries[0].status).toBe("needs_revalidation");
  expect(result.workspace.entries[0].inputs.memories).toEqual([ref]);
});
it("rechecks current records after an event history gap and preserves completed work", async () => {
  const assignment = {
    job: { spec: { brief: fixture.command.payload } },
    owner_id: "worker",
    epoch: 1,
  } as unknown as Assignment;
  const state = workspaceFromBrief(
    assignment.job.spec.brief,
    "2099-01-01T00:00:00Z",
    "2099-02-01T00:00:00Z",
  );
  state.entries[0].inputs.memories = [
    { memory_id: "missing", revision: 1, label: "Retired" },
  ];
  state.entries.push({
    ...structuredClone(state.entries[0]),
    id: "completed",
    status: "completed",
  });
  const host: HostCommands = {
    async request(command) {
      return command.action === "memory_changes"
        ? {
            kind: "changes",
            page: { cursor: 100, snapshot_required: true, changes: [] },
          }
        : { kind: "memories", memories: [] };
    },
  };
  const result = await reconcileActiveWork(host, assignment, state, 1);
  expect(result.snapshotRequired).toBe(true);
  expect(result.cursor).toBe(100);
  expect(result.workspace.entries[0].status).toBe("needs_revalidation");
  expect(result.workspace.entries.at(-1)?.status).toBe("completed");
});
