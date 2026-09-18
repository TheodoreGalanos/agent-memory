import { readFile, writeFile } from "node:fs/promises";
import { it, expect } from "vitest";
import { definition } from "../src/catalogue.js";
import { JevProvider, providerState } from "../src/providers.js";
import { questionMap, validateResponse } from "../src/validation.js";
import { packet, maximum } from "./fixtures.js";

// Explicit paid opt-in only; ordinary check/test runs never read the key.
it.skipIf(process.env.MEMORY_TEST_JEV !== "1")(
  "live bounded batch versus separate questions",
  async () => {
    const key = process.env.JEV_API_KEY;
    if (!key) throw new Error("JEV_API_KEY is required");
    const p = packet("J01");
    p.subject = "Synthetic stair-width claim";
    p.frame = "Authored test data, not a real building or safety determination";
    const fields = {
      candidate:
        "I infer that the stair is 1.2 metres wide; this is not a directly observed measurement.",
      claim: "The stair is 1.2 metres wide.",
      evidence:
        "Drawing A states that this stair is 1.2 metres wide. A later on-site measurement of the same stair records 1.0 metres. Both refer to the same measurement definition and location. The discrepancy is unresolved.",
    };
    p.evidence = Object.entries(fields).map(([name, content]) => ({
      ...p.evidence[0],
      name,
      content,
      pointer: `/${name}`,
      coverage: [name],
      origin: "agent_generated",
    }));
    p.questions.push({ ...p.questions[0], definition: definition("J02") });
    const provider = new JevProvider(
      "jev",
      "jev-latest",
      { ...maximum, tokens: 20000, cost_microunits: 1000 },
      () => key,
    );
    const inputPrice = 0.042 / 1_000_000;
    const reportPath = new URL(
      `../../../evals/judgement/${process.env.MEMORY_JEV_REPORT ?? "live-smoke.json"}`,
      import.meta.url,
    );
    const previous = JSON.parse(
      await readFile(reportPath, "utf8").catch((error) => {
        if (error.code === "ENOENT") return "{}";
        throw error;
      }),
    );
    const previousAttempts = [
      ...(previous.previousAttempts ?? []),
      ...(previous.calls ?? []),
    ];
    if (previousAttempts.length > 1)
      throw new Error(
        "Smoke request allowance already used; review the saved report before authorising a new run",
      );
    let reserved = previousAttempts.length * 20000 * inputPrice;
    const report: { [key: string]: unknown } = {
      date: new Date().toISOString(),
      purpose: "Live smoke only; no semantic qualification",
      model: provider.model,
      priceSource: "https://docs.typesafe.ai/cookbooks/parallel_questions",
      inputUsdPerMillion: 0.042,
      outputUsdPerMillion: 0,
      spendCapUsd: 0.01,
      state: providerState(p),
      questions: questionMap(p),
      definitionRevisions: Object.fromEntries(
        p.questions.map((q) => [q.definition.id, q.definition.revision]),
      ),
      billingNote:
        "Estimated from reported tokens and documented unit price; not an observed invoice.",
      previousAttempts,
      calls: [],
    };
    const calls = report.calls as Record<string, unknown>[];
    try {
      // One shared batch, then its two families separately; at most four total
      // calls including the retained rejected cookbook-model request.
      const selections = [p.questions, ...p.questions.map((q) => [q])];
      expect(selections.length + previousAttempts.length).toBeLessThanOrEqual(
        4,
      );
      for (const questions of selections) {
        const selected = { ...p, questions };
        const estimated =
          Buffer.byteLength(
            JSON.stringify({
              state: providerState(selected),
              questions: questionMap(selected),
              model: provider.model,
            }),
          ) +
          Object.keys(questionMap(selected)).length * 256;
        reserved += estimated * inputPrice;
        if (reserved > 0.01)
          throw new Error("Live smoke spending bound exceeded");
        const start = performance.now();
        const response = await provider.evaluate(
          selected,
          AbortSignal.timeout(20000),
        );
        const call = {
          requestedModel: provider.model,
          questions: Object.keys(questionMap(selected)),
          latencyMs: Math.round(performance.now() - start),
          status: response.status,
          raw: response.raw,
          usage: response.usage,
        };
        calls.push(call);
        expect(response.status).toBe(200);
        validateResponse(selected, response.raw, true);
      }
      const batch = validateResponse(p, calls[0].raw, true);
      const selectedChoices = Object.fromEntries(
        Object.entries(batch.answers).map(([k, a]) => [
          k,
          a.type === "choice" ? a.choice : a,
        ]),
      );
      report.batchChoices = selectedChoices;
      report.expectedChoices = {
        "J01.assessment": "inference",
        "J01.faithfulness": "yes",
        "J02.assessment": "both",
      };
      report.estimatedCostUsd = calls.reduce(
        (sum, c) =>
          sum +
          ((c.usage as { input_tokens?: number }).input_tokens ?? 0) *
            inputPrice,
        0,
      );
      report.batchAndSingleChoicesAgree = calls
        .slice(1)
        .every((c) =>
          Object.entries(
            (c.raw as { answers: Record<string, { choice: string }> }).answers,
          ).every(([k, a]) => selectedChoices[k] === a.choice),
        );
      report.matchesAuthoredExpectations = Object.entries(
        report.expectedChoices as Record<string, string>,
      ).every(([k, v]) => selectedChoices[k] === v);
    } finally {
      await writeFile(reportPath, JSON.stringify(report, null, 2) + "\n");
    }
  },
  90000,
);
