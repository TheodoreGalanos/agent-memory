import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { randomUUID } from "node:crypto";
import { loadEnvFile } from "node:process";
import { it, expect } from "vitest";
import { createModels } from "@earendil-works/pi-ai";
import { azureOpenAIResponsesProvider } from "@earendil-works/pi-ai/providers/azure-openai-responses";
import { BACKGROUND_CONTEXT as context } from "@earendil-works/pi-agent-core";
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type { FormationWindow } from "../../../contracts/generated/formation-window.js";
import type { JudgementPacket } from "../../../contracts/generated/judgement-packet.js";
import { HostClient } from "../../pi-worker/src/host-client.js";
import { openLocalSession } from "../../pi-worker/src/local-session.js";
import {
  GenerativeProvider,
  JevProvider,
  type JudgementProvider,
  type ProviderReply,
} from "../../judgement/src/providers.js";
import { questionMap } from "../../judgement/src/validation.js";
import { fenceFor } from "../../judgement/src/packet.js";
import { runFormation } from "../src/index.js";
import dataset from "../../../evals/formation/live/cases.json" with { type: "json" };

type Call = {
  provider: string;
  subject: string;
  packet: JudgementPacket;
  questions: unknown;
  latencyMs?: number;
  response?: ProviderReply;
  failure?: string;
};
// Disabled in ordinary checks. Rust owns the disposable host and supplied assignment.
it.skipIf(
  process.env.MEMORY_LIVE_FORMATION !== "1" ||
    !process.env.MEMORY_FORMATION_LIVE_FIXTURE,
)(
  "evaluates authored formation cases with Azure reference and Jev shadow",
  async () => {
    const reportPath = process.env.MEMORY_FORMATION_LIVE_REPORT;
    if (!reportPath)
      throw new Error("An explicit unused report path is required");
    const f = JSON.parse(
      await readFile(process.env.MEMORY_FORMATION_LIVE_FIXTURE!, "utf8"),
    ) as {
      assignment: Assignment;
      url: string;
      token: string;
      directory: string;
      sources: FormationWindow["source"][];
    };
    const report = {
      date: new Date().toISOString(),
      purpose: dataset.purpose,
      mode: "shadow",
      azureResponseFormat: "json_schema",
      callLimit: 40,
      estimatedAllowanceUsd: 1,
      pricing: {
        azureReference: {
          inputPerMillion: 0.4,
          outputPerMillion: 1.6,
          source: "https://developers.openai.com/api/docs/models/gpt-4.1-mini",
        },
        jev: {
          inputPerMillion: 0.042,
          outputPerMillion: 0,
          source: "https://docs.typesafe.ai/cookbooks/parallel_questions",
        },
        note: "Public list-price proxies, not an Azure or Jev invoice. Azure regional/contract pricing and Jev alias release may differ. Missing token telemetry is not zero.",
      },
      calls: [] as Call[],
      cases: [] as Record<string, unknown>[],
      failure: null as string | null,
    };
    await writeFile(reportPath, JSON.stringify(report, null, 2) + "\n", {
      flag: "wx",
    });
    const save = () =>
      writeFile(reportPath, JSON.stringify(report, null, 2) + "\n");
    loadEnvFile(".env");
    if (!process.env.JEV_API_KEY || !process.env.AZURE_OPENAI_API_KEY)
      throw new Error("Both provider credentials are required");
    const models = createModels();
    models.setProvider(azureOpenAIResponsesProvider());
    const model = models.getModel("azure-openai-responses", "gpt-4.1-mini");
    if (!model)
      throw new Error("Installed Pi catalog has no configured reference model");
    const maximum = {
      tokens: 40000,
      provider_calls: 1,
      cost_microunits: 25000,
      sandbox_time_ms: 0,
      sandbox_cpu_ms: 0,
      output_bytes: 0,
    };
    const azure = new GenerativeProvider(
      "azure-reference",
      models,
      model,
      maximum,
      2048,
    );
    const jev = new JevProvider(
      "jev",
      "jev-latest",
      maximum,
      () => process.env.JEV_API_KEY!,
    );
    function observe(provider: JudgementProvider): JudgementProvider {
      return {
        id: provider.id,
        model: provider.model,
        release: provider.release,
        maximum: provider.maximum,
        distributions: provider.distributions,
        async evaluate(packet, signal) {
          if (report.calls.length >= 40)
            throw new Error("Paid request limit reached");
          const call: Call = {
            provider: provider.id,
            subject: packet.subject,
            packet,
            questions: questionMap(packet),
          };
          report.calls.push(call);
          await save(); // Count an interrupted attempt before sending.
          const start = performance.now();
          try {
            call.response = await provider.evaluate(
              packet,
              AbortSignal.any([signal, AbortSignal.timeout(45000)]),
            );
            return call.response;
          } catch (error) {
            call.failure =
              error instanceof Error ? error.name : "ProviderError";
            throw error;
          } finally {
            call.latencyMs = Math.round(performance.now() - start);
            await save();
          }
        },
      };
    }
    const host = new HostClient(f.url, f.token),
      local = await openLocalSession(
        join(f.directory, "live-session"),
        f.assignment.job.session_id,
      );
    const options = {
      reference: observe(azure),
      jev: observe(jev),
      mode: "shadow" as const,
      maxAttempts: 1,
      disclosure: {
        policy: f.assignment.job.spec.brief.disclosure_policy,
        scope: f.assignment.job.spec.brief.scope,
        providers: ["azure-reference", "jev"],
      },
    };
    try {
      for (const [index, item] of dataset.cases.entries()) {
        await host.request({
          action: "renew",
          fence: fenceFor(f.assignment),
          lease_seconds: 300,
        });
        const start = performance.now(),
          from = report.calls.length;
        try {
          const result = await runFormation(
            host,
            f.assignment,
            local.session,
            {
              id: randomUUID(),
              source: f.sources[index],
              operation: "live-evaluation",
              limit: 32,
            },
            options,
            context,
          );
          const checks = Object.entries(item.expected).map(
            ([eventId, expected]) => {
              const record = result.records.find((r) =>
                r.record.label.endsWith(`: ${eventId}`),
              );
              const event = item.events.find((e) => e.event_id === eventId)!;
              return {
                eventId,
                expected,
                retained: !!record,
                retentionMatches: !!record === expected.retain,
                statusPreserved:
                  !record ||
                  record.record.evidential_status === event.evidential_status,
                userScopePreserved:
                  !record ||
                  !event.content.explicit_contribution ||
                  record.record.scope.user_id ===
                    f.assignment.job.spec.brief.scope.user_id,
                simulationHistorical:
                  !record ||
                  event.evidential_status !== "simulation" ||
                  record.record.availability === "historical",
              };
            },
          );
          report.cases.push({
            id: item.id,
            latencyMs: Math.round(performance.now() - start),
            callRange: [from, report.calls.length],
            checks,
            result,
          });
        } catch (error) {
          report.cases.push({
            id: item.id,
            latencyMs: Math.round(performance.now() - start),
            callRange: [from, report.calls.length],
            failure:
              error instanceof Error ? error.message : "Formation failed",
            expected: item.expected,
          });
        }
        await save();
        // Stop a broken connection/configuration instead of repeating paid failures across cases.
        const recent = report.calls.slice(from);
        if (recent.some((c) => c.failure || c.response?.status !== 200))
          throw new Error("Provider transport failed; stopped remaining cases");
      }
      expect(report.calls.length).toBeLessThanOrEqual(40);
      expect(report.cases).toHaveLength(dataset.cases.length);
    } catch (error) {
      report.failure =
        error instanceof Error ? error.message : "Live run failed";
      await save();
      throw error;
    } finally {
      await local.close();
      await save();
    }
  },
  1_500_000,
);
