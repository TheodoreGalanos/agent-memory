// ABOUTME: Task result validation: a structural default and a hook for domain checks.
import type { Validation, ValidationVerdict, WorkBrief, WorkResult } from "../../../contracts/generated/assignment.js";
import type { ToolEvent } from "../../../contracts/generated/formation-window.js";

export type { Validation, ValidationVerdict };

/** Validates a task result against its brief and recorded experience. The brief and execution
 * service own input and tool access; adapters do not grant or filter either. */
export interface TaskAdapter {
  id: string;
  /** Implement domain checks here when reported coverage alone is insufficient. */
  validate(brief: WorkBrief, result: WorkResult, experience: ToolEvent[]): Validation;
}

/** A structural check only: each criterion must appear verbatim in reported coverage or a
 * finding. This checks reporting consistency, not whether the work was performed correctly. */
export const criteriaAdapter: TaskAdapter = {
  id: "criteria",
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
