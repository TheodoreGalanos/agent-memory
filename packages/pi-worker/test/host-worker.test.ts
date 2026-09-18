import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { it, expect } from "vitest";
import {
  createModels,
  fauxProvider,
  fauxAssistantMessage,
  fauxToolCall,
} from "@earendil-works/pi-ai";
import { NodeExecutionEnv } from "@earendil-works/pi-agent-core/node";
import type {
  Assignment,
  WorkResult,
} from "../../../contracts/generated/assignment.js";
import { HostClient } from "../src/host-client.js";
import { openLocalSession } from "../src/local-session.js";
import { attachAssignedWorker } from "../src/assigned-worker.js";

// The Rust integration test owns the real host, credentials and seeded domain DB.
it.skipIf(!process.env.MEMORY_WORKER_FIXTURE)(
  "drives assigned Pi work through the real Rust HTTP coordinator",
  async () => {
    const fixture = JSON.parse(
      await readFile(process.env.MEMORY_WORKER_FIXTURE!, "utf8"),
    ) as {
      assignment: Assignment;
      url: string;
      token: string;
      directory: string;
    };
    const local = await openLocalSession(
      join(fixture.directory, "sessions"),
      fixture.assignment.job.session_id,
    );
    const faux = fauxProvider();
    const models = createModels();
    models.setProvider(faux.provider);
    await writeFile(
      join(fixture.directory, "evidence.txt"),
      "Inspected fixture evidence",
    );
    const result: WorkResult = {
      schema_version: "1",
      status: "complete",
      examined_scope: fixture.assignment.job.spec.brief.scope,
      inputs: fixture.assignment.job.spec.brief.inputs,
      findings: [],
      coverage: { examined: ["evidence.txt"], unexamined: [] },
      unresolved_work: [],
      proposed_changes: [],
      child_outputs: [],
      known_effects: [],
      usage: { status: "unknown" },
    };
    faux.setResponses([
      fauxAssistantMessage(fauxToolCall("read", { path: "evidence.txt" }), {
        stopReason: "toolUse",
      }),
      fauxAssistantMessage(JSON.stringify(result)),
    ]);
    const worker = await attachAssignedWorker(
      fixture.assignment,
      new HostClient(fixture.url, fixture.token),
      {
        ...fixture.assignment.job.spec.brief.profile,
        maxPayloadBytes: 64_000,
        maxOutputTokens: faux.getModel().maxTokens,
        providerReservation: {
          tokens: 64_000 + faux.getModel().maxTokens,
          provider_calls: 1,
          cost_microunits: 0,
          sandbox_cpu_ms: 0,
          sandbox_time_ms: 0,
          output_bytes: 0,
        },
      },
      {
        session: local.session,
        models,
        model: faux.getModel(),
        env: new NodeExecutionEnv({ cwd: fixture.directory }),
        tools: ["read"],
      },
    );
    try {
      const settled = await worker.run();
      expect(settled).toMatchObject({
        kind: "settled",
        job: { state: "completed" },
      });
      if (settled.kind !== "settled") throw new Error("No result");
      expect(settled.job.result?.result_artifact).toBeTruthy();
      expect(faux.state.callCount).toBe(2);
    } finally {
      await worker.close();
      await local.close();
    }
  },
);
