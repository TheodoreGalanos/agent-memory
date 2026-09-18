// Offline analysis only: no credentials, providers or database writes.
import { readFileSync, writeFileSync } from "node:fs";
import {
  distributionSumTolerance,
  validateResponse,
} from "../packages/judgement/src/validation.ts";

const path = process.argv[2];
if (!path)
  throw new Error(
    "Usage: node --experimental-strip-types scripts/summarize-formation-live.ts REPORT.json [SUMMARY.json]",
  );
const report = JSON.parse(readFileSync(path, "utf8"));
const cases = JSON.parse(
  readFileSync(
    new URL("../evals/formation/live/cases.json", import.meta.url),
    "utf8",
  ),
).cases;
const events = new Map(
  cases.flatMap((c: any) =>
    c.events.map((event: any) => [
      event.event_id,
      { event, expected: c.expected[event.event_id] },
    ]),
  ),
);
const checks = report.cases.flatMap((c: any) => c.checks ?? []);
const providers: Record<string, any> = {};
for (const call of report.calls) {
  const eventId = call.subject.replace(/^Formation of /, "");
  const { event, expected } = events.get(eventId) as any;
  const p = (providers[call.provider] ??= {
    calls: 0,
    valid: 0,
    invalid: [],
    latenciesMs: [],
    inputTokens: 0,
    outputTokens: 0,
    tokenTelemetryComplete: true,
    returnedModels: [],
    counterfactual: [],
  });
  p.calls++;
  p.latenciesMs.push(call.latencyMs);
  const usage = call.response?.usage;
  if (usage?.input_tokens == null || usage?.output_tokens == null)
    p.tokenTelemetryComplete = false;
  else {
    p.inputTokens += usage.input_tokens;
    p.outputTokens += usage.output_tokens;
  }
  const model = call.response?.raw?.model;
  if (model && !p.returnedModels.includes(model)) p.returnedModels.push(model);
  try {
    const result = validateResponse(
      call.packet,
      call.response.raw,
      call.provider === "jev",
    );
    p.valid++;
    const choices = Object.fromEntries(
      Object.entries(result.answers).map(([k, a]) => [
        k,
        a.type === "choice" ? a.choice : null,
      ]),
    );
    const families = call.packet.questions.map((q: any) => q.definition.id);
    // Apply the existing host's formation disposition to valid answers only.
    // Jev results are hypothetical outcomes: it did not author the stored records.
    const retained =
      choices["J01.assessment"] === event.evidential_status &&
      choices["J01.faithfulness"] === "yes" &&
      choices["J02.assessment"] === "supports" &&
      (!families.includes("J03") ||
        ["boundary", "correction", "continuation"].includes(
          choices["J03.assessment"]!,
        )) &&
      (!families.includes("J04") ||
        [
          "local_convention",
          "conditional_method",
          "untested_generalisation",
        ].includes(choices["J04.assessment"]!)) &&
      (!families.includes("J05") ||
        choices["J05.assessment"] === "commitment") &&
      (!families.includes("J27") ||
        [
          "task_local",
          "persistent_preference",
          "proposed_shared_rule",
        ].includes(choices["J27.assessment"]!));
    p.counterfactual.push({
      eventId,
      choices,
      expectedRetain: expected.retain,
      retained,
      matches: retained === expected.retain,
    });
  } catch (error) {
    p.invalid.push({
      eventId,
      reason: error instanceof Error ? error.message : "Invalid response",
      category: call.response?.raw?.invalid_json
        ? "invalid_json"
        : "answer_contract",
    });
  }
}
for (const [id, p] of Object.entries(providers)) {
  const sorted = p.latenciesMs
    .filter(Number.isFinite)
    .toSorted((a: number, b: number) => a - b);
  const middle = Math.floor(sorted.length / 2);
  p.medianLatencyMs =
    sorted.length === 0
      ? null
      : sorted.length % 2
        ? sorted[middle]
        : (sorted[middle - 1] + sorted[middle]) / 2;
  p.estimatedCostUsd = p.tokenTelemetryComplete
    ? (p.inputTokens * (id === "jev" ? 0.042 : 0.4) +
        p.outputTokens * (id === "jev" ? 0 : 1.6)) /
      1e6
    : null;
  p.counterfactualMatches = p.counterfactual.filter(
    (x: any) => x.matches,
  ).length;
}
const summary = {
  distributionSumTolerance,
  purpose:
    "One bounded live diagnostic; not blinded accuracy or provider qualification.",
  cases: report.cases.length,
  events: checks.length,
  calls: report.calls.length,
  actualFormation: {
    expectedRetentions: checks.filter((c: any) => c.expected.retain).length,
    retained: checks.filter((c: any) => c.retained).length,
    falseRetentions: checks
      .filter((c: any) => c.retained && !c.expected.retain)
      .map((c: any) => c.eventId),
    missedRetentions: checks
      .filter((c: any) => !c.retained && c.expected.retain)
      .map((c: any) => c.eventId),
    matched: checks.filter((c: any) => c.retentionMatches).length,
    caseFailures: report.cases.filter((c: any) => c.failure),
    persistedRecords: report.persistedRecords?.length,
    retainedStatusesPreserved: checks
      .filter((c: any) => c.retained)
      .every((c: any) => c.statusPreserved),
    retainedUserScopesPreserved: checks
      .filter((c: any) => c.retained)
      .every((c: any) => c.userScopePreserved),
  },
  providers,
  limitations: [
    "Fresh authored cases; no held-out statistical calibration.",
    "Descriptive event IDs and case context were visible to providers, which can cue decisions.",
    "Both providers saw the same packets; agreement is not independent evidence of correctness.",
    "Jev ran in shadow mode. Its retention outcomes are offline counterfactuals, not persisted writes.",
    "Azure model identity is reported by the Pi adapter; an immutable deployed model snapshot was not attested.",
    "Costs use public list-price proxies, not invoices.",
    "The cautious-inference expectation needs review alongside the support-question wording.",
  ],
};
writeFileSync(
  process.argv[3] ?? path.replace(/\.json$/, ".summary.json"),
  JSON.stringify(summary, null, 2) + "\n",
);
console.log(
  JSON.stringify(
    {
      cases: summary.cases,
      events: summary.events,
      calls: summary.calls,
      actualFormation: summary.actualFormation,
      providers: Object.fromEntries(
        Object.entries(providers).map(([id, p]) => [
          id,
          {
            valid: p.valid,
            invalid: p.invalid.length,
            counterfactualMatches: p.counterfactualMatches,
            medianLatencyMs: p.medianLatencyMs,
            inputTokens: p.inputTokens,
            outputTokens: p.outputTokens,
            estimatedCostUsd: p.estimatedCostUsd,
            returnedModels: p.returnedModels,
          },
        ]),
      ),
    },
    null,
    2,
  ),
);
