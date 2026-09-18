import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { randomUUID } from "node:crypto";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { expect, it } from "vitest";
import { BACKGROUND_CONTEXT as context } from "@earendil-works/pi-agent-core";
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type {
  ConsolidationWindow,
  ConsolidationProposal,
  MemoryRef,
} from "../../../contracts/generated/consolidation-review.js";
import type { JudgementProvider } from "../../judgement/src/index.js";
import { definition } from "../../judgement/src/index.js";
import { fenceFor } from "../../judgement/src/packet.js";
import { HostClient } from "../../pi-worker/src/host-client.js";
import {
  InterpreterClient,
  type InterpreterReply,
} from "../../pi-worker/src/interpreter-client.js";
import { openLocalSession } from "../../pi-worker/src/local-session.js";
import { runConsolidation } from "../src/index.js";
import { runQualification, executeProcedure } from "../src/qualification.js";

it.skipIf(!process.env.MEMORY_CONSOLIDATION_FIXTURE)(
  "preserves exceptions, groups sources and qualifies advisory/executable transfer through child jobs",
  async () => {
    const f = JSON.parse(
      await readFile(process.env.MEMORY_CONSOLIDATION_FIXTURE!, "utf8"),
    ) as {
      assignment: Assignment;
      url: string;
      token: string;
      worker_token: string;
      directory: string;
      cases: MemoryRef[];
      suite: string;
      overlap_suite: string;
      code: string;
    };
    const admin = new HostClient(f.url, f.token),
      host = new HostClient(f.url, f.worker_token),
      fence = fenceFor(f.assignment);
    let local = await openLocalSession(
      join(f.directory, "parent-session"),
      f.assignment.job.session_id,
    );
    const brief = f.assignment.job.spec.brief;
    let calls = 0,
      syntheses = 0;
    const provider: JudgementProvider = {
      id: "scripted-reference",
      model: "consolidation-fixture",
      release: "1",
      distributions: false,
      maximum: {
        tokens: 10000,
        provider_calls: 1,
        cost_microunits: 0,
        output_bytes: 0,
        sandbox_time_ms: 0,
        sandbox_cpu_ms: 0,
      },
      async evaluate(packet) {
        calls++;
        const bad = JSON.stringify(packet.evidence).includes(
          "universal causal rule",
        );
        const answers = Object.fromEntries(
          packet.questions.flatMap(({ definition: d }) =>
            d.questions.map((q) => [
              `${d.id}.${q.key}`,
              { type: "choice", choice: bad ? "no" : "yes" },
            ]),
          ),
        );
        return {
          status: 200,
          raw: { model: "consolidation-fixture", answers },
          usage: { input_tokens: 10, output_tokens: 5, cost_microunits: 0 },
        };
      },
    };
    const criterion = {
      ...definition("J08"),
      id: "pressure-location",
      permitted_uses: ["investigation"],
    };
    const proposal = (
      w: ConsolidationWindow,
      executable = false,
    ): ConsolidationProposal => ({
      window_id: w.id,
      label: executable
        ? "Pressure lookup function"
        : "Pressure inspection playbook",
      scope: w.scope,
      concise_summary: "Read rated_pressure on the instance.",
      invariant:
        "Inspect the source convention before choosing the property location",
      variations: ["Value may be on instance or related type"],
      untested: ["Other source conventions"],
      clauses: [
        {
          text: "Inspect instance, then related type when instance lacks the property",
          conditions: ["instance convention", "type convention"],
          uncertainty: ["Other conventions untested"],
          support: w.cases.map((c) => c.reference),
          exceptions: w.exceptions,
        },
      ],
      procedure: {
        family: "procedure",
        purpose: "Find rated pressure with source evidence",
        capabilities: executable ? ["python"] : ["read"],
        applicability: ["Instance or related-type property convention"],
        exclusions: ["No property on instance or related type"],
        counterexamples: w.exceptions,
        method: executable
          ? {
              form: "executable",
              artifact_id: f.code,
              entrypoint: "find_pressure",
              inputs: ["instance", "type"],
              outputs: ["value", "location"],
            }
          : {
              form: "advisory",
              steps: [
                "Inspect instance.rated_pressure",
                "If absent, inspect related type.rated_pressure",
              ],
              evidence_criteria: ["Return value and inspected location"],
            },
        contract: {
          parameters: {
            instance: "Instance fields",
            type: "Related type fields",
          },
          effects: ["Read supplied data"],
          replay_class: "reconcile",
          decision_points: ["Is the property absent on the instance?"],
          stopping_criteria: [
            "Value and location are supported, or both locations are exhausted",
          ],
          checks: ["Check that the returned field is rated_pressure"],
          recognition: [
            {
              definition: criterion,
              model: "fixture",
              model_release: "1",
              policy: brief.policy,
            },
          ],
        },
      },
    });
    const selection = {
      purpose: "Find rated pressure",
      mechanism: "Inspect property placement",
      outcomes: ["Verified location"],
      source_context: "Related source conventions",
      members: f.cases.slice(0, 2),
    };
    const options = {
      reference: provider,
      maxAttempts: 1,
      disclosure: {
        policy: brief.disclosure_policy,
        scope: brief.scope,
        providers: [provider.id],
      },
      synthesize: async (w: ConsolidationWindow) => {
        syntheses++;
        return proposal(w);
      },
    };
    try {
      const input = { id: randomUUID(), selection };
      const retained = await runConsolidation(
        host,
        f.assignment,
        local.session,
        input,
        options,
        context,
      );
      expect(retained.deferred).toEqual([]);
      expect(retained.independent_sources).toBe(2);
      expect(retained.record?.record.qualification.status).toBe("candidate");
      expect(retained.record?.record.derived_from).toHaveLength(3);
      expect(retained.record?.record.scope.project_id).toBe(
        brief.scope.project_id,
      );
      if (retained.record?.record.content.family !== "procedure")
        throw new Error("No procedure retained");
      expect(retained.record.record.content.counterexamples).toContainEqual(
        f.cases[2],
      );
      await local.close();
      const resumed = await openLocalSession(
        join(f.directory, "parent-session"),
        f.assignment.job.session_id,
      );
      local.session = resumed.session;
      local.close = resumed.close;
      const before = calls;
      expect(
        (
          await runConsolidation(
            host,
            f.assignment,
            local.session,
            input,
            options,
            context,
          )
        ).record?.reference,
      ).toEqual(retained.record.reference);
      expect(calls).toBe(before);
      expect(syntheses).toBe(1);
      const response = await host.request({
        action: "consolidation_window",
        fence,
        id: input.id,
        selection,
      });
      if (response.kind !== "consolidation_window")
        throw new Error("No cohort");
      expect(response.window.cases).toHaveLength(3);
      expect(
        response.window.source_groups.map((g) => g.members.length).sort(),
      ).toEqual([1, 2]);
      expect(response.window.exceptions).toEqual([f.cases[2]]);
      await expect(
        host.request({
          action: "review_consolidation",
          fence,
          id: randomUUID(),
          proposal: {
            ...proposal(response.window),
            scope: { ...brief.scope, project_id: null },
          },
        }),
      ).rejects.toThrow();
      const missingCounterexample = proposal(response.window);
      if (missingCounterexample.procedure?.family === "procedure")
        missingCounterexample.procedure.counterexamples = [];
      await expect(
        host.request({
          action: "review_consolidation",
          fence,
          id: randomUUID(),
          proposal: missingCounterexample,
        }),
      ).rejects.toThrow();
      const noException = await runConsolidation(
        host,
        f.assignment,
        local.session,
        { id: randomUUID(), selection },
        {
          ...options,
          synthesize: async (w) => {
            const p = proposal(w);
            p.clauses[0].exceptions = [];
            return p;
          },
        },
        context,
      );
      expect(noException.record).toBeNull();
      expect(noException.deferred.join()).toContain("Exception");
      const duplicate = await runConsolidation(
        host,
        f.assignment,
        local.session,
        {
          id: randomUUID(),
          selection: { ...selection, members: [f.cases[1]] },
        },
        options,
        context,
      );
      expect(duplicate.record).toBeNull();
      expect(duplicate.independent_sources).toBe(1);
      const overgeneral = await runConsolidation(
        host,
        f.assignment,
        local.session,
        { id: randomUUID(), selection },
        {
          ...options,
          synthesize: async (w) => ({
            ...proposal(w),
            invariant: "universal causal rule",
          }),
        },
        context,
      );
      expect(overgeneral.record).toBeNull();
      await expect(
        host.request({
          action: "start_qualification",
          fence,
          request_id: randomUUID(),
          review_id: retained.review_id,
          suite_id: f.overlap_suite,
        }),
      ).rejects.toThrow();
      const compare = async (reviewId: string, fail = false) => {
        const request = {
          action: "start_qualification" as const,
          fence,
          request_id: randomUUID(),
          review_id: reviewId,
          suite_id: f.suite,
        };
        const started = await host.request(request);
        if (started.kind !== "job") throw new Error("No qualification job");
        const repeated = await host.request(request);
        expect(repeated.kind === "job" && repeated.job.id).toBe(started.job.id);
        await expect(
          host.request({
            action: "adopt_procedure",
            fence,
            evaluation_job: started.job.id,
          }),
        ).rejects.toThrow();
        const claimed = await admin.request({
          action: "claim",
          job_id: started.job.id,
          lease_seconds: 300,
        });
        if (claimed.kind !== "assignment")
          throw new Error("No evaluation assignment");
        const assignment = claimed.assignment;
        await admin.request({ action: "start", fence: fenceFor(assignment) });
        const session = await openLocalSession(
          join(f.directory, started.job.id),
          started.job.session_id,
        );
        const transport = {
          async interpreter(
            request: Record<string, unknown>,
          ): Promise<InterpreterReply> {
            const { stdout } = await promisify(execFile)(
              "python3",
              [
                "-c",
                "import json,sys,interpreter; print(json.dumps(interpreter.request(sys.argv[1],json.loads(sys.argv[2]))))",
                f.directory,
                JSON.stringify(request),
              ],
              { cwd: "services/harbor-bridge" },
            );
            return JSON.parse(stdout);
          },
        };
        const interpreter = new InterpreterClient(
          transport,
          admin,
          assignment,
          randomUUID(),
        );
        await interpreter.start(context);
        let result;
        let executions = 0;
        try {
          result = await runQualification(
            admin,
            assignment,
            session.session,
            async (t, c) => {
              executions++;
              const task = t.task as Record<string, Record<string, number>>;
              let answer: unknown = null;
              const trajectory: string[] = [];
              if (
                t.arm === "procedure" &&
                !Array.isArray(t.representation) &&
                typeof t.representation !== "string" &&
                t.representation.record.content.family === "procedure" &&
                t.representation.record.content.method.form === "executable"
              ) {
                answer = await executeProcedure(
                  admin,
                  assignment,
                  interpreter,
                  t.representation,
                  t.operationId,
                  task,
                  c,
                );
                trajectory.push("Executed assigned Python procedure");
              } else {
                const plan =
                  typeof t.representation === "string"
                    ? ["instance"]
                    : Array.isArray(t.representation)
                      ? ["instance", "type"]
                      : t.representation.record.content.family ===
                            "procedure" &&
                          t.representation.record.content.method.form ===
                            "advisory"
                        ? t.representation.record.content.method.steps.map(
                            (s) =>
                              s.includes("related type") ? "type" : "instance",
                          )
                        : [];
                for (const location of plan) {
                  trajectory.push(`Read ${location}.rated_pressure`);
                  if (task[location]?.rated_pressure !== undefined) {
                    answer = { value: task[location].rated_pressure, location };
                    break;
                  }
                }
              }
              if (fail && t.arm === "procedure") answer = null;
              return {
                answer,
                trajectory,
                cost_microunits: 0,
                recognition: [
                  {
                    criterion: "pressure-location",
                    packet_valid: true,
                    answer_correct: true,
                    routing_correct: !fail,
                    false_negative: fail,
                  },
                ],
              };
            },
            context,
          );
          const resumed = await runQualification(
            admin,
            assignment,
            session.session,
            async () => {
              throw new Error("Completed trials must not execute on resume");
            },
            context,
          );
          expect(resumed.child_outputs).toEqual(result.child_outputs);
          expect(executions).toBe(6);
        } finally {
          await interpreter.stop(context);
          await session.close();
        }
        await admin.request({
          action: "complete",
          request_id: randomUUID(),
          fence: fenceFor(assignment),
          result: result!,
        });
        const adopted = await host.request({
          action: "adopt_procedure",
          fence,
          evaluation_job: started.job.id,
        });
        if (adopted.kind !== "adoption_result")
          throw new Error("No adoption result");
        expect(adopted.result.adopted).toBe(!fail);
        if (
          !fail &&
          adopted.result.candidate.record.content.family === "procedure"
        ) {
          expect(
            adopted.result.candidate.record.content.applicability.join(),
          ).toContain("Task fits a tested context:");
        }
        expect(adopted.result.rates.summary).toBe(0.5);
        expect(adopted.result.recognition["pressure-location"]).toEqual({
          question: true,
          model: !fail,
          policy: !fail,
        });
        expect(adopted.result.candidate.record.qualification.status).toBe(
          fail ? "candidate" : "evaluated",
        );
        expect(
          await host.request({
            action: "adopt_procedure",
            fence,
            evaluation_job: started.job.id,
          }),
        ).toEqual(adopted);
        return adopted.result;
      };
      const advisory = await compare(retained.review_id);
      const executable = await runConsolidation(
        host,
        f.assignment,
        local.session,
        { id: randomUUID(), selection },
        { ...options, synthesize: async (w) => proposal(w, true) },
        context,
      );
      expect(executable.deferred).toEqual([]);
      const executed = await compare(executable.review_id);
      const failing = await runConsolidation(
        host,
        f.assignment,
        local.session,
        { id: randomUUID(), selection },
        options,
        context,
      );
      const deferred = await compare(failing.review_id, true);
      expect(deferred.reasons.join()).toContain("below policy");
      if (process.env.MEMORY_CONSOLIDATION_REPORT)
        await writeFile(
          process.env.MEMORY_CONSOLIDATION_REPORT,
          JSON.stringify(
            {
              profile:
                "authored scripted qualification with real Host, Pi and local Python interpreter",
              advisory: advisory.rates,
              executable: executed.rates,
              failedCandidate: deferred.rates,
              sourceGroups: 2,
              cohortCases: 3,
              limitation:
                "Functional transfer fixture; no live synthesis/judgement model or production sandbox qualification",
            },
            null,
            2,
          ) + "\n",
        );
    } finally {
      await local.close();
    }
  },
  120_000,
);
