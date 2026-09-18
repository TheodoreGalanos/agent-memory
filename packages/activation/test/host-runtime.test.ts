import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { randomUUID } from "node:crypto";
import { expect, it, vi } from "vitest";
import {
  BACKGROUND_CONTEXT as context,
  value,
} from "@earendil-works/pi-agent-core";
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type { ActivationQuery } from "../../../contracts/generated/activation-window.js";
import { HostClient } from "../../pi-worker/src/host-client.js";
import { openLocalSession } from "../../pi-worker/src/local-session.js";
import {
  workspaceFromBrief,
  briefAccess,
} from "../../pi-worker/src/workspace.js";
import {
  runActivation,
  applyActivation,
  activationAccess,
} from "../src/index.js";
import probes from "../../../evals/activation/next-step-probes.json" with { type: "json" };
import { fenceFor } from "../../judgement/src/packet.js";
import type { JudgementProvider } from "../../judgement/src/index.js";

it.skipIf(!process.env.MEMORY_ACTIVATION_FIXTURE)(
  "retrieves and judges full methods, keeps exceptions and delivers workspace context",
  async () => {
    const f = JSON.parse(
      await readFile(process.env.MEMORY_ACTIVATION_FIXTURE!, "utf8"),
    ) as {
      assignment: Assignment;
      url: string;
      token: string;
      directory: string;
      executable: { memory_id: string; revision: number; label: string };
      method: { memory_id: string; revision: number; label: string };
      exception: { memory_id: string; revision: number; label: string };
    };
    const host = new HostClient(f.url, f.token),
      brief = f.assignment.job.spec.brief;
    const local = await openLocalSession(
      join(f.directory, "sessions"),
      f.assignment.job.session_id,
    );
    let calls = 0;
    const reference: JudgementProvider = {
      id: "scripted-reference",
      model: "activation-fixture",
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
        const question = packet.evidence.find((e) =>
          ["current_question", "question"].includes(e.name),
        );
        if (question) {
          expect(typeof question.content).toBe("string");
          expect(packet.evidence.some((e) => e.name === "task_context")).toBe(
            true,
          );
        }
        const family = packet.questions[0].definition.id;
        if (family === "J07" || family === "J08") {
          const method = JSON.stringify(packet.evidence);
          expect(method).toContain(
            method.includes("Torque calculation")
              ? "return 42"
              : "Close the inlet valve",
          );
        }
        const condition = JSON.stringify(
          packet.evidence.find((e) => e.name === "prerequisites")?.content ??
            "",
        );
        const supplied = packet.evidence.find(
          (e) => e.name === "evidence",
        )?.content;
        const facts = supplied
          ? (JSON.parse(String(supplied)) as { power: string; bypass: string })
          : undefined;
        const conditionAnswer = condition.includes("Bypass")
          ? facts?.bypass === "closed"
            ? "established"
            : facts?.bypass === "open"
              ? "unmet"
              : "insufficient_evidence"
          : facts?.power === "off"
            ? "established"
            : "unmet";
        const answers = Object.fromEntries(
          packet.questions.flatMap(({ definition: d }) =>
            d.questions.map((q) => [
              `${d.id}.${q.key}`,
              {
                type: "choice",
                choice:
                  d.id === "J06"
                    ? q.key === "direct" || q.key === "exception"
                      ? "yes"
                      : "no"
                    : d.id === "J07"
                      ? "relevant"
                      : conditionAnswer,
              },
            ]),
          ),
        );
        return {
          status: 200,
          raw: { model: "activation-fixture", answers },
          usage: { input_tokens: 10, output_tokens: 5, cost_microunits: 0 },
        };
      },
    };
    const query: ActivationQuery = {
      scope: brief.scope,
      question: "How do I isolate the pump?",
      task_context: JSON.stringify({ power: "off", bypass: "unknown" }),
      entities: [],
      exact: [],
      families: [],
      existing: [],
      scan_limit: 100,
      candidate_limit: 8,
      traversal_limit: 20,
      context_bytes: 20000,
    };
    const options = {
      reference,
      embeddings: {
        document: vi.fn(),
        query: vi.fn(async () => ({
          identity: {
            model: "authored-test-vectors",
            revision: "1",
            dimensions: 2,
            representation: "memory-content-v1",
          },
          values: [1, 0],
        })),
      },
      maxAttempts: 1,
      disclosure: {
        policy: brief.disclosure_policy,
        scope: brief.scope,
        providers: [reference.id],
      },
    };
    try {
      const input = { id: randomUUID(), query };
      const result = await runActivation(
        host,
        f.assignment,
        local.session,
        input,
        options,
        context,
      );
      expect(options.embeddings.query).toHaveBeenCalledOnce();
      expect(result.window.query.vector?.values).toEqual([1, 0]);
      expect(result.selected.map((r) => r.memory_id).sort()).toEqual(
        [f.method.memory_id, f.exception.memory_id].sort(),
      );
      expect(result.window.groups).toHaveLength(1);
      expect(result.window.groups[0].unresolved_conflict).toBe(true);
      expect(
        result.dispositions.find(
          (d) => d.memory.memory_id === f.method.memory_id,
        ),
      ).toMatchObject({
        applicability: "needs_investigation",
        missing_conditions: [
          "This exclusion does not apply: Bypass valve is open",
        ],
      });
      await writeFile(
        join(f.directory, "activation-result.json"),
        JSON.stringify({ window: result.window.id }),
      );
      const fence = fenceFor(f.assignment);
      const unchecked = await host.request({
        action: "select_activation",
        fence,
        selection: { window_id: result.window.id, decisions: {} },
      });
      expect(unchecked.kind).toBe("context_package");
      if (unchecked.kind === "context_package")
        expect(unchecked.package.selected).toEqual([]);
      await expect(
        host.request({ action: "embedding_inputs", limit: 1 }),
      ).rejects.toThrow();
      const saved = (await local.session.getValue(
        value<{ decisions: Record<string, string> }>(
          "memory.activation",
          input.id,
        ),
        context,
      ))!.value;
      await expect(
        host.request({
          action: "select_activation",
          fence,
          selection: {
            window_id: result.window.id,
            decisions: { "1/J06": saved.decisions["0/J06"] },
          },
        }),
      ).rejects.toThrow();
      const before = calls;
      expect(
        (
          await runActivation(
            host,
            f.assignment,
            local.session,
            input,
            options,
            context,
          )
        ).selected,
      ).toEqual(result.selected);
      expect(calls).toBe(before);
      expect(options.embeddings.query).toHaveBeenCalledOnce();
      const workspace = applyActivation(
        workspaceFromBrief(
          brief,
          f.assignment.job.deadline,
          f.assignment.job.spec.retain_until,
        ),
        result,
      );
      const access = activationAccess(briefAccess(brief), result);
      const entries = workspace.entries.filter((e) => e.kind === "memory");
      expect(entries).toHaveLength(2);
      expect(entries.every((e) => access.allows(e.scope, e.inputs))).toBe(true);
      expect(workspace.context_groups).toHaveLength(1);
      expect(workspace.conflicts).toHaveLength(1);
      expect(JSON.stringify(entries)).toContain("separate isolation step");
      const revoked = activationAccess(
        { revision: "revoked", allows: () => false },
        result,
      );
      expect(entries.some((e) => revoked.allows(e.scope, e.inputs))).toBe(
        false,
      );
      const continuation = (packageResult: typeof result | undefined) => {
        if (!packageResult?.selected.length) return "retrieve_evidence";
        const method = packageResult.dispositions.find(
          (d) => d.memory.memory_id === f.method.memory_id,
        )!;
        if (method.applicability === "needs_investigation")
          return "inspect_prerequisites";
        if (method.applicability === "inapplicable") return "avoid_method";
        return "review_candidate_method";
      };
      for (const probe of probes.cases) {
        const activated = await runActivation(
          host,
          f.assignment,
          local.session,
          {
            id: randomUUID(),
            query: {
              ...query,
              task_context: JSON.stringify(probe.task_context),
            },
          },
          options,
          context,
        );
        expect(continuation(undefined)).not.toBe(probe.expected_next_step);
        expect(continuation(activated), probe.id).toBe(
          probe.expected_next_step,
        );
      }
      const executable = await runActivation(
        host,
        f.assignment,
        local.session,
        {
          id: randomUUID(),
          query: {
            ...query,
            question: "Torque",
            exact: [f.executable.memory_id],
          },
        },
        options,
        context,
      );
      expect(executable.selected.map((r) => r.memory_id)).toEqual([
        f.executable.memory_id,
      ]);
      expect(executable.window.candidates[0].definition).toContain("return 42");
      const executableWorkspace = applyActivation(
        workspaceFromBrief(
          brief,
          f.assignment.job.deadline,
          f.assignment.job.spec.retain_until,
        ),
        executable,
      );
      expect(
        executableWorkspace.entries.find((e) => e.kind === "memory")!.text,
      ).toContain("return 42");
      // A small allowance cannot silently select the method without its exception.
      const small = await runActivation(
        host,
        f.assignment,
        local.session,
        { id: randomUUID(), query: { ...query, context_bytes: 1 } },
        options,
        context,
      );
      expect(small.selected).toEqual([]);
      expect(small.deferred.join(" ")).toContain("complete group exceeds");
      // Existing inspected context permits an explicit empty addition.
      const existing = await runActivation(
        host,
        f.assignment,
        local.session,
        { id: randomUUID(), query: { ...query, existing: result.selected } },
        options,
        context,
      );
      expect(existing.selected).toEqual([]);
      expect(existing.reason).toContain("Existing context is sufficient");
    } finally {
      await local.close();
    }
  },
  60000,
);
