// ABOUTME: Job-scoped judgement policy shared by pool processes and application tools.
import type { WorkBrief } from "../../../contracts/generated/work-command.js";
import { catalogueResponses } from "../../judgement/src/responses.js";
import type { JudgementOptions } from "../../judgement/src/runtime.js";
import type { Services } from "./composition.js";
import { observeProvider } from "./trace.js";

export function judgementFor(
  service: NonNullable<Services["judgement"]>,
  brief: WorkBrief,
  log: (line: string) => void,
): JudgementOptions {
  const { primary, fallback, comparison } = service;
  return {
    primary: observeProvider(primary, "primary", log),
    fallback: fallback ? observeProvider(fallback, "fallback", log) : undefined,
    comparison: comparison ? { provider: observeProvider(comparison.provider, "comparison", log) } : undefined,
    maxAttempts: 1,
    responses: catalogueResponses,
    disclosure: {
      policy: brief.disclosure_policy,
      scope: brief.scope,
      providers: [primary.id, ...(fallback ? [fallback.id] : []), ...(comparison ? [comparison.provider.id] : [])],
    },
  };
}
