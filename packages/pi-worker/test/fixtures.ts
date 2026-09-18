import { readFileSync } from "node:fs";
import type { Command } from "../../../contracts/generated/work-command.js";
import type { WorkResult } from "../../../contracts/generated/work-result.js";

export const checkpoints = [
  "before-correction",
  "after-correction",
  "revision-d",
] as const;
export type Checkpoint = (typeof checkpoints)[number];
export interface Evidence {
  id: string;
  observed_at: string;
  source: { source_id: string; revision: string };
  query: string;
  rows?: unknown[];
  result?: unknown;
}

export function loadJson(path: string): unknown {
  return JSON.parse(
    readFileSync(new URL(`../../../${path}`, import.meta.url), "utf8"),
  );
}

export function input(name: Checkpoint): {
  command: Command;
  evidence: Evidence[];
} {
  return loadJson(`evals/property-location/inputs/${name}.json`) as ReturnType<
    typeof input
  >;
}

export function scriptedResult(name: Checkpoint): WorkResult {
  return loadJson(`evals/property-location/results/${name}.json`) as WorkResult;
}
