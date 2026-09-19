// ABOUTME: Tests for worker.json handling: the Jev-first judgement shape is validated, legacy
// ABOUTME: reference/jev/mode keys migrate explicitly and deterministically, and credential needs follow the routes.
import { expect, it } from "vitest";
import { credentialsRequired, migrateWorkerConfiguration, type WorkerConfiguration } from "../src/configuration.js";

const base = { sessions_directory: "pool", task_model: { provider: "azure-openai-responses", model: "gpt-5.4-mini" } };

it("accepts the current shape unchanged and defaults the primary route to Jev", () => {
  const { config, migrated } = migrateWorkerConfiguration({ ...base, judgement: { primary: { kind: "jev", model: "jev-latest" } } });
  expect(migrated).toBe(false);
  expect(config.judgement.primary).toEqual({ kind: "jev", model: "jev-latest" });
  const defaulted = migrateWorkerConfiguration({ ...base });
  expect(defaulted.config.judgement.primary).toEqual({ kind: "jev", model: "jev-latest" });
  expect(defaulted.config.judgement.fallback).toBeUndefined();
});

it("migrates shadow and qualified configurations to Jev primary with the former reference as fallback", () => {
  for (const mode of ["shadow", "qualified", undefined]) {
    const { config, migrated, notes } = migrateWorkerConfiguration({
      ...base, reference_model: { provider: "azure-openai-responses", model: "gpt-4.1-mini" }, jev: { model: "jev-latest" }, judgement_mode: mode,
      judgement_maximum: { tokens: 20_000, cost_microunits: 10_000 },
    });
    expect(migrated).toBe(true);
    expect(config.judgement).toEqual({
      primary: { kind: "jev", model: "jev-latest" },
      fallback: { kind: "generative", provider: "azure-openai-responses", model: "gpt-4.1-mini" },
      maximum: { tokens: 20_000, cost_microunits: 10_000 },
    });
    expect(notes.join(" ")).toMatch(/reference_model/);
    expect("reference_model" in config).toBe(false);
  }
});

it("migrates a reference-only configuration to a generative primary, since no Jev route was configured", () => {
  const { config } = migrateWorkerConfiguration({ ...base, reference_model: { provider: "azure-openai-responses", model: "gpt-4.1-mini" }, judgement_mode: "reference" });
  expect(config.judgement).toEqual({ primary: { kind: "generative", provider: "azure-openai-responses", model: "gpt-4.1-mini" } });
  const explicit = migrateWorkerConfiguration({ ...base, reference_model: { provider: "azure-openai-responses", model: "gpt-4.1-mini" }, jev: { model: "jev-latest" }, judgement_mode: "reference" });
  // An explicit reference mode kept the conventional model operational; Jev observed. Jev-first inverts that.
  expect(explicit.config.judgement.primary).toEqual({ kind: "jev", model: "jev-latest" });
});

it("rejects mixing legacy and current judgement keys, and identical primary and fallback routes", () => {
  expect(() => migrateWorkerConfiguration({ ...base, reference_model: { provider: "p", model: "m" }, judgement: { primary: { kind: "jev", model: "x" } } })).toThrow(/legacy/);
  expect(() => migrateWorkerConfiguration({ ...base, judgement: { primary: { kind: "jev", model: "x" }, fallback: { kind: "jev", model: "x" } } })).toThrow(/distinct/);
});

it("requires only the credentials the configured routes use", () => {
  const jevOnly: WorkerConfiguration = { ...base, judgement: { primary: { kind: "jev", model: "jev-latest" } } };
  expect(credentialsRequired(jevOnly, ["formation"])).toEqual(["JEV_API_KEY"]);
  expect(credentialsRequired(jevOnly, ["investigation", "formation"])).toEqual(["AZURE_OPENAI_API_KEY", "JEV_API_KEY"]);
  const withFallback: WorkerConfiguration = { ...jevOnly, judgement: { ...jevOnly.judgement, fallback: { kind: "generative", provider: "azure-openai-responses", model: "gpt-4.1-mini" } } };
  expect(credentialsRequired(withFallback, ["formation"])).toEqual(["AZURE_OPENAI_API_KEY", "JEV_API_KEY"]);
});
