// ABOUTME: Pure helpers for the live walkthrough: the findings-to-tool-events connector,
// ABOUTME: the WorkResult template given to the model, and compact terminal formatting.
import type { Job } from "../contracts/generated/host-response.js";
import type { WorkResult } from "../contracts/generated/work-result.js";
import type { RenderManifest } from "../contracts/generated/render-manifest.js";
import type { ToolEvent } from "../contracts/generated/formation-window.js";
import type { FormationResult } from "../contracts/generated/formation-result.js";
import type { JudgementUsage } from "../contracts/generated/formation-result.js";

export {
  deliverInputMemories,
  workResultOutputSchema,
  workResultTemplate,
} from "../packages/supervisor/src/task.js";
export { formatReply } from "../packages/supervisor/src/trace.js";

export interface ModelReference {
  provider: string;
  model: string;
}

/** Each published finding becomes one formation capture. Nothing is verified or reinterpreted here. */
export function findingsToToolEvents(job: Job, result: WorkResult, model: ModelReference): ToolEvent[] {
  const observed_at = new Date().toISOString();
  return result.findings.map((finding, index) => ({
    event_id: `finding-${index + 1}`,
    observed_at,
    kind: "capture",
    origin: finding.origin,
    evidential_status: finding.evidential_status,
    content: {
      episode: job.id,
      objective: job.spec.brief.purpose,
      conditions: [
        `Published WorkResult of job ${job.id}`,
        `Produced by ${model.provider}/${model.model} under profile ${job.spec.brief.profile.label}`,
      ],
      boundary_explicit: true,
      retention: "optional",
      explicitly_selected: true,
      explicit_contribution: false,
      actor_id: null,
      content: {
        family: "knowledge",
        statement: finding.statement,
        subject: null,
        predicate: null,
        uncertainty: result.coverage.unexamined,
        examined_coverage: result.coverage.examined,
      },
      evidence: { finding, supporting_memories: finding.supporting_memories },
      source_locators: [],
      corrects: [],
      uncertainty: result.unresolved_work,
      coverage: result.coverage,
      based_on: [],
    },
  }));
}

const clip = (text: string, length = 80) => (text.length > length ? `${text.slice(0, length - 1)}…` : text);

export function formatManifest(manifest: RenderManifest): string {
  const selected = manifest.selected.map((entry) => `${entry.kind} "${clip(entry.text, 60)}"`).join(", ");
  const parts = [
    `render ${manifest.status} (${manifest.provider}/${manifest.model})`,
    `${manifest.selected.length} selected${selected ? `: ${selected}` : ""}`,
    `${manifest.sources.length} source(s)`,
    `${manifest.conflicts.length} conflict group(s)`,
    `${manifest.deferred.length} deferred`,
    manifest.final_payload_bytes == null ? "payload not checked" : `${manifest.final_payload_bytes} bytes`,
  ];
  const usage = manifest.usage;
  if (usage.uncached_input_tokens != null || usage.output_tokens != null)
    parts.push(`tokens in ${usage.uncached_input_tokens ?? "?"} / out ${usage.output_tokens ?? "?"}`);
  return `  ${parts.join("; ")}`;
}

export function summarizeFormation(result: FormationResult): string {
  const lines = [`Retained ${result.records.length} candidate record(s):`];
  for (const item of result.records)
    lines.push(`  + ${item.record.label} r${item.reference.revision} [${item.record.content.family}, ${item.record.evidential_status}, ${item.record.qualification.status}]`);
  if (result.deferred.length) {
    lines.push(`Deferred ${result.deferred.length}:`);
    for (const item of result.deferred) lines.push(`  - ${item.event_id}: ${item.reason}${item.required ? " (required)" : ""}`);
  }
  const unresolved = [...new Set(result.unresolved)];
  if (unresolved.length) lines.push(`Unresolved: ${unresolved.join("; ")}`);
  lines.push(`Coverage: examined ${result.coverage.examined.length}, unexamined ${result.coverage.unexamined.length}`);
  const usage = Object.values(result.judgement_usage);
  const sum = (key: keyof JudgementUsage) => {
    const known = usage.map((u) => u[key]).filter((v): v is number => typeof v === "number");
    return known.length === usage.length ? String(known.reduce((a, b) => a + b, 0)) : known.length ? `${known.reduce((a, b) => a + b, 0)} (partially unknown)` : "unknown";
  };
  lines.push(`Judgement usage over ${usage.length} assessment(s): input tokens ${sum("input_tokens")}, output tokens ${sum("output_tokens")}, cost microunits ${sum("cost_microunits")}`);
  return lines.join("\n");
}
