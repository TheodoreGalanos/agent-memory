import type { Assignment } from "../../../contracts/generated/assignment.js";
import type { HostCommands } from "../../pi-worker/src/host-client.js";
import {
  emptyInputs,
  reconcileWorkspace,
  type WorkspaceState,
  type WorkspaceChange,
} from "../../pi-worker/src/workspace.js";
import { fenceFor } from "../../judgement/src/packet.js";

/** Call at a work checkpoint and save the returned workspace and cursor together. */
export async function reconcileActiveWork(
  host: HostCommands,
  assignment: Assignment,
  state: WorkspaceState,
  cursor: number,
  signal?: AbortSignal,
  now = Date.now(),
): Promise<{
  workspace: WorkspaceState;
  cursor: number;
  snapshotRequired: boolean;
}> {
  const fence = fenceFor(assignment);
  const response = await host.request(
    { action: "memory_changes", fence, after: cursor, limit: 100 },
    signal,
  );
  if (response.kind !== "changes")
    throw new Error("Host did not return memory changes");
  const page = response.page;
  const changes: WorkspaceChange[] = page.changes.map((n) => ({
    previous: {
      ...emptyInputs(),
      memories: [
        n.previous,
        ...state.entries
          .flatMap((e) => e.inputs.memories)
          .filter(
            (r) =>
              r.memory_id === n.previous.memory_id &&
              r.revision < n.previous.revision,
          ),
      ],
    },
    kind:
      n.kind === "wording"
        ? "wording"
        : n.kind === "access" || n.kind === "support_removal"
          ? "access"
          : "substantive",
    scope: n.scope,
    validTime: n.valid_time,
  }));
  if (page.snapshot_required) {
    const references = [
      ...new Map(
        state.entries
          .filter((e) => e.status !== "completed")
          .flatMap((e) => e.inputs.memories)
          .map((r) => [`${r.memory_id}/${r.revision}`, r]),
      ).values(),
    ];
    for (let i = 0; i < references.length; i += 100) {
      const batch = references.slice(i, i + 100);
      const current = await host.request(
        { action: "current_memories", fence, references: batch },
        signal,
      );
      if (current.kind !== "memories")
        throw new Error("Host did not return current memories");
      for (const ref of batch) {
        const m = current.memories.find(
          (m) => m.reference.memory_id === ref.memory_id,
        );
        if (
          !m ||
          m.reference.revision !== ref.revision ||
          m.record.availability !== "routine"
        )
          changes.push({
            previous: { ...emptyInputs(), memories: [ref] },
            kind: "access",
            scope: state.scope,
            validTime: { kind: "unknown" },
          });
      }
    }
  }
  return {
    workspace: reconcileWorkspace(state, changes, now),
    cursor: page.cursor,
    snapshotRequired: page.snapshot_required,
  };
}
