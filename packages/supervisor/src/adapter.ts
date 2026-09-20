// ABOUTME: An independent output validator for a task: checks a settled result against its brief
// ABOUTME: without trusting the model's own claims, and reports a durable verdict before it commits.
import type { Validation, ValidationVerdict, WorkBrief, WorkResult } from "../../../contracts/generated/assignment.js";
import type { ToolEvent } from "../../../contracts/generated/formation-window.js";

export type { Validation, ValidationVerdict };

/** One process-owned task adapter: what the model may see, which tools it may use, what to observe
 * from its experience for the record, and an independent check of its settled result. `validate`
 * never trusts `result`'s own claims about itself (status, coverage) as proof; it checks them. */
export interface TaskAdapter {
  id: string;
  version: string;
  /** Public inputs the model may see for this brief. */
  inputs(brief: WorkBrief): unknown;
  /** Tool names this adapter permits for the brief. */
  tools(brief: WorkBrief): string[];
  /** Observations extracted from the task's experience events, kept alongside the verdict. */
  observe(events: ToolEvent[]): string[];
  /** An independent check of the settled result against the brief; never derived from the result's
   * own status or coverage claims alone. */
  validate(brief: WorkBrief, result: WorkResult, experience: ToolEvent[]): Validation;
}

/** Validates structurally against `brief.output_criteria`: each criterion must appear, verbatim, in
 * the result's own examined coverage or a finding's statement (evidence the task actually produced,
 * not merely asserted status). A blocked task cannot be assessed either way. A complete status with
 * unresolved work left is inconsistent and fails regardless of criteria coverage. */
export const criteriaAdapter: TaskAdapter = {
  id: "criteria",
  version: "1",
  inputs: (brief) => brief.inputs,
  tools: (brief) => brief.capabilities.tools,
  observe: (events) =>
    events
      .filter((event) => event.kind === "tool_result" || event.kind === "message")
      .map((event) => `${event.kind}: ${JSON.stringify(event.content).slice(0, 200)}`),
  validate(brief, result) {
    if (result.status === "blocked")
      return { verdict: "unknown", reasons: ["Task was blocked before its output criteria could be assessed"] };
    const covered = [...result.coverage.examined, ...result.findings.map((f) => f.statement)].join("\n");
    const unmet = brief.output_criteria.filter((criterion) => !covered.includes(criterion));
    const reasons: string[] = [];
    if (unmet.length) reasons.push(`Unmet output criteria: ${unmet.join("; ")}`);
    if (result.status === "complete" && result.unresolved_work.length)
      reasons.push(`Unresolved work remains despite a complete status: ${result.unresolved_work.join("; ")}`);
    return reasons.length ? { verdict: "failed", reasons } : { verdict: "passed", reasons: [] };
  },
};

/** Selects the task adapter for a brief: an installed adapter whose id matches the brief's profile
 * label, or the deterministic default otherwise. The brief carries no dedicated adapter-selection
 * field today, so the profile label (already a per-task configuration reference) is the simplest
 * available signal; this is a decision point future briefs may replace with an explicit field. */
export function selectAdapter(brief: WorkBrief, adapters: TaskAdapter[] = []): TaskAdapter {
  return adapters.find((adapter) => adapter.id === brief.profile.label) ?? criteriaAdapter;
}
