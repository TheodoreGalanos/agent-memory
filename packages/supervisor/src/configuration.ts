// ABOUTME: worker.json shape for the pool: task model, Jev-first judgement routes (primary, optional fallback and
// ABOUTME: comparison) and the explicit one-time migration from the former reference/jev/mode keys.
import type { SandboxBinding } from "../../pi-worker/src/asp-execution-env.js";
import type { SupportedProcess } from "./index.js";

export type JudgementRoute =
  | { kind: "jev"; model: string }
  | { kind: "generative"; provider: string; model: string };

export interface JudgementConfiguration {
  /** The operational provider for every configured structured judgement. Jev by default. */
  primary: JudgementRoute;
  /** Consulted only when the primary yields no usable answer. */
  fallback?: JudgementRoute;
  /** Separately budgeted evaluation work recorded after each decision; never selected. */
  comparison?: JudgementRoute;
  maximum?: { tokens: number; cost_microunits: number };
}

export interface WorkerConfiguration {
  /** Path to a dotenv file with provider credentials; relative paths resolve from the config file. */
  env_file?: string;
  sessions_directory: string;
  concurrency?: number | { interactive?: number; deferred?: number };
  poll_ms?: number;
  lease_seconds?: number;
  /** How long running work may finish after SIGTERM before it is cancelled. */
  drain_ms?: number;
  max_payload_bytes?: number;
  task_reservation_cost_microunits?: number;
  processes?: SupportedProcess[];
  task_model: { provider: string; model: string };
  judgement: JudgementConfiguration;
  /** Where investigation's granted native tools run. Absent means investigation tasks are tool-less. */
  execution?:
    | { kind: "node"; cwd?: string }
    | { kind: "asp"; binding: SandboxBinding; helper_path?: string };
}

/** Keys from the reference-plus-shadow arrangement this shape replaces. */
interface LegacyJudgementKeys {
  reference_model?: { provider: string; model: string };
  jev?: { model: string } | null;
  judgement_mode?: "reference" | "shadow" | "qualified";
  judgement_maximum?: { tokens: number; cost_microunits: number };
}
const LEGACY_KEYS = ["reference_model", "jev", "judgement_mode", "judgement_maximum"] as const;
export const DEFAULT_JEV_MODEL = "jev-latest";

const sameRoute = (a: JudgementRoute, b: JudgementRoute) => JSON.stringify(a) === JSON.stringify(b);

/** Returns the current shape. Legacy keys migrate deterministically: a configured Jev becomes the primary and
 * the former reference model its fallback; without Jev the reference model stays operational as primary. */
export function migrateWorkerConfiguration(
  raw: Record<string, unknown>,
): { config: WorkerConfiguration; migrated: boolean; notes: string[] } {
  const legacy = raw as LegacyJudgementKeys;
  const present = LEGACY_KEYS.filter((key) => raw[key] !== undefined);
  if (present.length && raw.judgement !== undefined)
    throw new Error(`worker.json mixes legacy keys (${present.join(", ")}) with a judgement block; remove the legacy keys`);
  const notes: string[] = [];
  let judgement: JudgementConfiguration;
  if (present.length) {
    const reference: JudgementRoute | undefined = legacy.reference_model
      ? { kind: "generative", provider: legacy.reference_model.provider, model: legacy.reference_model.model }
      : undefined;
    const jev: JudgementRoute | undefined = legacy.jev ? { kind: "jev", model: legacy.jev.model } : undefined;
    if (!jev && !reference) throw new Error("Legacy judgement keys name neither a reference model nor Jev");
    judgement = jev
      ? { primary: jev, ...(reference ? { fallback: reference } : {}) }
      : { primary: reference! };
    if (legacy.judgement_maximum) judgement.maximum = legacy.judgement_maximum;
    notes.push(
      `Migrated legacy keys ${present.join(", ")} (mode ${legacy.judgement_mode ?? "default"}) to judgement ${JSON.stringify(judgement)}; ` +
        `the former reference_model is ${reference && jev ? "now the fallback" : reference ? "the primary because no Jev route was configured" : "absent"}.`,
    );
  } else {
    judgement = (raw.judgement as JudgementConfiguration | undefined) ?? { primary: { kind: "jev", model: DEFAULT_JEV_MODEL } };
    if (!judgement.primary) throw new Error("judgement.primary is required");
  }
  if (judgement.fallback && sameRoute(judgement.fallback, judgement.primary))
    throw new Error("judgement.primary and judgement.fallback must be distinct routes");
  if (judgement.comparison && sameRoute(judgement.comparison, judgement.primary))
    throw new Error("judgement.primary and judgement.comparison must be distinct routes");
  const config = Object.fromEntries(Object.entries(raw).filter(([key]) => !(LEGACY_KEYS as readonly string[]).includes(key))) as unknown as WorkerConfiguration;
  config.judgement = judgement;
  return { config, migrated: present.length > 0, notes };
}

/** Environment variables the composed processes need: Azure for any generative model in use, Jev for any Jev route.
 * An `asp` execution service needs no additional environment credential: its SSH identity is a trusted file
 * reference carried in the binding itself, not an ambient secret. */
export function credentialsRequired(config: WorkerConfiguration, processes: SupportedProcess[]): string[] {
  const judgementProcesses: SupportedProcess[] = ["formation", "consolidation", "maintenance", "activation"];
  const routes = processes.some((p) => judgementProcesses.includes(p))
    ? [config.judgement.primary, config.judgement.fallback, config.judgement.comparison].filter((r): r is JudgementRoute => !!r)
    : [];
  const required = new Set<string>();
  if (processes.includes("investigation") || processes.includes("consolidation") || processes.includes("evaluation") || routes.some((r) => r.kind === "generative")) required.add("AZURE_OPENAI_API_KEY");
  if (routes.some((r) => r.kind === "jev")) required.add("JEV_API_KEY");
  return [...required].sort();
}
