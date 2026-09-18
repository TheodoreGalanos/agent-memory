import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { randomUUID } from "node:crypto";
import { expect, it } from "vitest";
import { BACKGROUND_CONTEXT as context } from "@earendil-works/pi-agent-core";
import type {
  Assignment,
  WorkResult,
} from "../../../contracts/generated/assignment.js";
import type {
  IntentionOccurrence,
  MemoryVersion,
} from "../../../contracts/generated/intention-occurrence.js";
import { HostClient } from "../../pi-worker/src/host-client.js";
import { openLocalSession } from "../../pi-worker/src/local-session.js";
import { workspaceFromBrief } from "../../pi-worker/src/workspace.js";
import { fenceFor } from "../../judgement/src/packet.js";
import type { JudgementProvider } from "../../judgement/src/index.js";
import { runMaintenance, runIntentionCheck } from "../src/index.js";
import { reconcileActiveWork } from "../src/workspace.js";
it.skipIf(!process.env.MEMORY_MAINTENANCE_FIXTURE)(
  "applies a correction, refreshes unfinished work and checks intention evidence through Host and Pi",
  async () => {
    const f = JSON.parse(
      await readFile(process.env.MEMORY_MAINTENANCE_FIXTURE!, "utf8"),
    ) as {
      assignment: Assignment;
      next_job: string;
      activation_job: string;
      activation_worker_token: string;
      semantic: IntentionOccurrence;
      before: MemoryVersion;
      occurrence: IntentionOccurrence;
      url: string;
      token: string;
      worker_token: string;
      next_worker_token: string;
      directory: string;
      result: WorkResult;
    };
    const admin = new HostClient(f.url, f.token);
    let host = new HostClient(f.url, f.worker_token);
    let assignment = f.assignment;
    let local = await openLocalSession(
      join(f.directory, "session"),
      assignment.job.session_id,
    );
    let ready = false,
      satisfied = false,
      calls = 0;
    const provider: JudgementProvider = {
      id: "scripted-reference",
      model: "maintenance-fixture",
      release: "1",
      distributions: false,
      maximum: {
        tokens: 100,
        provider_calls: 1,
        cost_microunits: 0,
        output_bytes: 0,
        sandbox_time_ms: 0,
        sandbox_cpu_ms: 0,
      },
      async evaluate(packet) {
        calls++;
        const choice = (id: string) =>
          ({
            "fixture-trigger": "affected",
            J13: "correction",
            J14: "compatible",
            J15: "none",
            J16: "unaffected",
            J08: ready ? "established" : "insufficient",
            J17: satisfied ? "satisfied" : "not_satisfied",
          })[id] ?? "unaffected";
        const answers = Object.fromEntries(
          packet.questions.flatMap(({ definition: d }) =>
            d.questions.map((q) => [
              `${d.id}.${q.key}`,
              { type: "choice", choice: choice(d.id) },
            ]),
          ),
        );
        return {
          status: 200,
          raw: { model: "maintenance-fixture", answers },
          usage: { input_tokens: 10, output_tokens: 5, cost_microunits: 0 },
        };
      },
    };
    const options = {
      reference: provider,
      maxAttempts: 1,
      disclosure: {
        policy: assignment.job.spec.brief.disclosure_policy,
        scope: assignment.job.spec.brief.scope,
        providers: [provider.id],
      },
    };
    try {
      const after = structuredClone(f.before.record);
      after.label = "Corrected property location";
      const input = {
        id: randomUUID(),
        request: {
          before: f.before.reference,
          after,
          removed_sources: [],
          candidates: [],
          reason: "Correct a mistaken lookup",
        },
      };
      const result = await runMaintenance(
        host,
        assignment,
        local.session,
        input,
        options,
        context,
      );
      expect(result.kind).toBe("correction");
      expect(result.changed[0].reference.revision).toBe(2);
      const count = calls;
      expect(
        (
          await runMaintenance(
            host,
            assignment,
            local.session,
            input,
            options,
            context,
          )
        ).changed[0].reference,
      ).toEqual(result.changed[0].reference);
      expect(calls).toBe(count);
      const workspace = workspaceFromBrief(
        assignment.job.spec.brief,
        assignment.job.deadline,
        assignment.job.spec.retain_until,
      );
      const memory = workspace.entries.find((e) =>
        e.inputs.memories.some(
          (m) => m.memory_id === f.before.reference.memory_id,
        ),
      )!;
      workspace.entries.push(
        {
          ...structuredClone(memory),
          id: "completed-conclusion",
          status: "completed",
        },
        {
          ...structuredClone(memory),
          id: "dependent",
          inputs: { sources: [], memories: [], artifacts: [] },
          supporting_entries: [memory.id],
        },
      );
      const refreshed = await reconcileActiveWork(
        host,
        assignment,
        workspace,
        0,
      );
      expect(
        refreshed.workspace.entries.find((e) => e.id === memory.id)?.status,
      ).toBe("needs_revalidation");
      expect(
        refreshed.workspace.entries.find((e) => e.id === "dependent")?.status,
      ).toBe("needs_revalidation");
      expect(
        refreshed.workspace.entries.find((e) => e.id === "completed-conclusion")
          ?.status,
      ).toBe("completed");
      expect(
        refreshed.workspace.entries.find((e) => e.id === "goal")?.status,
      ).toBe("active");
      await expect(
        runIntentionCheck(
          host,
          assignment,
          local.session,
          {
            id: randomUUID(),
            occurrence_id: f.occurrence.id,
            kind: { kind: "readiness" },
          },
          options,
          context,
        ),
      ).rejects.toThrow();
      ready = true;
      expect(
        (
          await runIntentionCheck(
            host,
            assignment,
            local.session,
            {
              id: randomUUID(),
              occurrence_id: f.occurrence.id,
              kind: { kind: "readiness" },
            },
            options,
            context,
          )
        ).state,
      ).toBe("armed");
      await admin.request({ action: "sweep_intentions", limit: 100 });
      const inspected = await admin.request({
        action: "inspect_intentions",
        definition_id: f.occurrence.definition.reference.memory_id,
      });
      if (inspected.kind !== "intentions") throw new Error("No occurrences");
      const fired = inspected.occurrences[0];
      expect(fired.state).toBe("fired");
      const done = { ...f.result, inputs: assignment.job.spec.brief.inputs };
      await host.request({
        action: "complete",
        fence: fenceFor(assignment),
        request_id: randomUUID(),
        result: done,
      });
      const execution = await admin.request({
        action: "claim",
        job_id: fired.job_id!,
        lease_seconds: 300,
      });
      if (execution.kind !== "assignment") throw new Error("No execution");
      await admin.request({
        action: "complete",
        fence: fenceFor(execution.assignment),
        request_id: randomUUID(),
        result: f.result,
      });
      await local.close();
      const next = await admin.request({
        action: "claim",
        job_id: f.next_job,
        lease_seconds: 300,
      });
      if (next.kind !== "assignment")
        throw new Error("No maintenance assignment");
      assignment = next.assignment;
      host = new HostClient(f.url, f.next_worker_token);
      local = await openLocalSession(
        join(f.directory, "session"),
        assignment.job.session_id,
      );
      const completion = () =>
        runIntentionCheck(
          host,
          assignment,
          local.session,
          {
            id: randomUUID(),
            occurrence_id: fired.id,
            kind: { kind: "completion" },
          },
          options,
          context,
        );
      expect((await completion()).state).toBe("fired");
      satisfied = true;
      expect((await completion()).state).toBe("fired");
      await admin.request({
        action: "confirm_intention",
        occurrence_id: fired.id,
      });
      expect((await completion()).state).toBe("completed");
      expect((await completion()).state).toBe("completed");
      await host.request({
        action: "complete",
        fence: fenceFor(assignment),
        request_id: randomUUID(),
        result: { ...f.result, inputs: assignment.job.spec.brief.inputs },
      });
      await local.close();
      const activation = await admin.request({
        action: "claim",
        job_id: f.activation_job,
        lease_seconds: 300,
      });
      if (activation.kind !== "assignment")
        throw new Error("No activation assignment");
      assignment = activation.assignment;
      host = new HostClient(f.url, f.activation_worker_token);
      local = await openLocalSession(
        join(f.directory, "session"),
        assignment.job.session_id,
      );
      const events = await admin.request({
        action: "events",
        after: 0,
        limit: 100,
      });
      if (events.kind !== "events") throw new Error("No events");
      const event = events.page.events.find(
        (e) => e.kind === "memory_maintained",
      )!;
      expect(
        (
          await runIntentionCheck(
            host,
            assignment,
            local.session,
            {
              id: randomUUID(),
              occurrence_id: f.semantic.id,
              kind: { kind: "trigger", event_id: event.id },
            },
            options,
            context,
          )
        ).state,
      ).toBe("fired");
    } finally {
      await local.close();
    }
  },
  60000,
);
