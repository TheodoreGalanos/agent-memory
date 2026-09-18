import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { it, expect } from "vitest";
import {
  BACKGROUND_CONTEXT as context,
  value,
} from "@earendil-works/pi-agent-core";
import {
  createModels,
  fauxProvider,
  fauxAssistantMessage,
} from "@earendil-works/pi-ai";
import { NodeExecutionEnv } from "@earendil-works/pi-agent-core/node";
import type {
  Assignment,
  WorkResult,
} from "../../../contracts/generated/assignment.js";
import { HostClient } from "../src/host-client.js";
import { openLocalSession } from "../src/local-session.js";
import { attachAssignedWorker } from "../src/assigned-worker.js";
import {
  inspectJson,
  type InvestigationPlan,
} from "../src/scoped-operation.js";

it.skipIf(!process.env.MEMORY_INVESTIGATION_FIXTURE)(
  "expands a definition, yields, and preserves an independent child's counterexample after reopening",
  async () => {
    const fixture = JSON.parse(
      await readFile(process.env.MEMORY_INVESTIGATION_FIXTURE!, "utf8"),
    ) as {
      assignment: Assignment;
      url: string;
      token: string;
      schedulerToken: string;
      directory: string;
    };
    const host = new HostClient(fixture.url, fixture.token);
    // Test scheduler also drives children. Parent calls always use its job-scoped credential.
    const scheduler = new HostClient(fixture.url, fixture.schedulerToken);
    const brief = fixture.assignment.job.spec.brief;
    const plan: InvestigationPlan = {
      method: {
        id: brief.profile.id,
        revision: 1,
        label: "Clearance comparison",
      },
      interpretation_conditions: [
        "Temporary works count toward the clearance requirement",
      ],
      definitions: [
        {
          name: "Required clearance",
          artifact_id: brief.inputs.artifacts[0],
          pointer: "/definitions/clearance",
        },
      ],
      steps: [
        "Check the permanent works",
        "Check temporary works for a counterexample",
      ].map((question, i) => ({
        key: String(i),
        question,
        inputs: brief.inputs,
        output_criteria: ["Report clearance and any missing coverage"],
        tools: [],
      })),
    };
    function result(assignment: Assignment, statement?: string): WorkResult {
      return {
        schema_version: "1",
        status: "complete",
        examined_scope: assignment.job.spec.brief.scope,
        inputs: assignment.job.spec.brief.inputs,
        findings: statement
          ? [
              {
                statement,
                origin: "agent_generated",
                evidential_status: "inference",
                applicability: assignment.job.spec.brief.scope,
                sources: [],
                supporting_memories: [],
                challenging_memories: [],
              },
            ]
          : [],
        coverage: {
          examined: [assignment.job.spec.brief.purpose],
          unexamined: [],
        },
        unresolved_work: [],
        proposed_changes: [],
        child_outputs: [],
        known_effects: [],
        usage: { status: "unknown" },
      };
    }
    async function drive(
      assignment: Assignment,
      client: HostClient,
      answer: WorkResult,
      investigation?: InvestigationPlan,
    ) {
      const local = await openLocalSession(
        join(fixture.directory, "investigation-sessions"),
        assignment.job.session_id,
      );
      const faux = fauxProvider();
      const models = createModels();
      models.setProvider(faux.provider);
      faux.setResponses([fauxAssistantMessage(JSON.stringify(answer))]);
      const worker = await attachAssignedWorker(
        assignment,
        client,
        {
          ...assignment.job.spec.brief.profile,
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
          tools: [],
          investigation,
        },
      );
      try {
        const outcome = await worker.run();
        const state = (
          await local.session.getValue(
            value<{ children: string[]; bundleId: string }>(
              "memory.investigation",
              assignment.job.operation_id,
            ),
            context,
          )
        )?.value;
        return { outcome, state, calls: faux.state.callCount };
      } finally {
        await worker.close();
        await local.close();
      }
    }
    const first = await drive(
      fixture.assignment,
      host,
      result(fixture.assignment),
      plan,
    );
    expect(first.outcome.kind).toBe("waiting");
    expect(first.calls).toBe(0);
    expect(first.state?.children).toHaveLength(2);
    const sessionIds = new Set([fixture.assignment.job.session_id]);
    for (const [i, id] of first.state!.children.entries()) {
      const claimed = await scheduler.request({
        action: "claim",
        job_id: id,
        lease_seconds: 30,
      });
      if (claimed.kind !== "assignment") throw new Error("Child not assigned");
      const assignment = claimed.assignment;
      sessionIds.add(assignment.job.session_id);
      expect(assignment.job.spec.brief.definitions.join(" ")).toContain(
        "including temporary works",
      );
      const answer = result(
        assignment,
        i === 0
          ? "Permanent works have 4 metres clearance"
          : "Counterexample: temporary works leave only 2 metres clearance",
      );
      if (i === 1) {
        answer.status = "partial";
        answer.coverage.unexamined = ["Night shift layout"];
        answer.unresolved_work = ["Night shift measurements are unavailable"];
      }
      expect((await drive(assignment, scheduler, answer)).outcome.kind).toBe(
        "settled",
      );
    }
    expect(sessionIds.size).toBe(3);
    await scheduler.request({ action: "recover" });
    const resumed = await scheduler.request({
      action: "claim",
      job_id: fixture.assignment.job.id,
      lease_seconds: 30,
    });
    if (resumed.kind !== "assignment") throw new Error("Parent not resumed");
    expect(resumed.assignment.epoch).toBeGreaterThan(fixture.assignment.epoch);
    // Parent intentionally omits both the counterexample and the missing evidence.
    const final = await drive(
      resumed.assignment,
      host,
      result(resumed.assignment, "Permanent layout meets the requirement"),
      plan,
    );
    expect(final.calls).toBe(1);
    expect(final.outcome).toMatchObject({
      kind: "settled",
      job: { state: "partial" },
    });
    if (final.outcome.kind !== "settled") throw new Error("No final result");
    const output = final.outcome.job.result!;
    expect(output.findings.some((f) => f.statement.includes("2 metres"))).toBe(
      true,
    );
    expect(output.coverage.unexamined.join(" ")).toContain("Night shift");
    expect(output.child_outputs).toHaveLength(3);
    expect(output.result_artifact).toBeTruthy();
    // Scoped reading is lease-bound, including after successful publication.
    await expect(
      inspectJson(
        host,
        {
          job_id: fixture.assignment.job.id,
          owner_id: resumed.assignment.owner_id,
          epoch: resumed.assignment.epoch,
        },
        brief.inputs.artifacts[0],
        "",
        context,
      ),
    ).rejects.toThrow();
  },
);
