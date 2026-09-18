// ABOUTME: Process entry for a worker pool child started by the Host. Reads its pool identity from
// ABOUTME: the environment and provider settings from a worker configuration file; runs until SIGTERM.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { loadEnvFile } from "node:process";
import { BACKGROUND_CONTEXT, withAbortSignal } from "@earendil-works/pi-agent-core";
import { createModels } from "@earendil-works/pi-ai";
import { azureOpenAIResponsesProvider } from "@earendil-works/pi-ai/providers/azure-openai-responses";
import { HostClient } from "../../pi-worker/src/host-client.js";
import { GenerativeProvider, JevProvider, type JudgementProvider } from "../../judgement/src/providers.js";
import { SUPPORTED_PROCESSES, runSupervisor, type SupportedProcess } from "./index.js";

interface WorkerConfiguration {
  /** Path to a dotenv file with provider credentials; relative paths resolve from the config file. */
  env_file?: string;
  sessions_directory: string;
  concurrency?: number | { interactive?: number; deferred?: number };
  poll_ms?: number;
  lease_seconds?: number;
  max_payload_bytes?: number;
  task_reservation_cost_microunits?: number;
  processes?: SupportedProcess[];
  task_model: { provider: string; model: string };
  reference_model: { provider: string; model: string };
  jev?: { model: string } | null;
  judgement_mode?: "reference" | "shadow";
  judgement_maximum?: { tokens: number; cost_microunits: number };
}

async function main() {
  const configPath = process.argv[2];
  if (!configPath) throw new Error("Usage: supervisor WORKER_CONFIG.json (pool identity comes from the environment)");
  const hostUrl = process.env.MEMORY_HOST_URL, token = process.env.MEMORY_POOL_TOKEN;
  if (!hostUrl || !token) throw new Error("MEMORY_HOST_URL and MEMORY_POOL_TOKEN are required");
  const config = JSON.parse(readFileSync(configPath, "utf8")) as WorkerConfiguration;
  const base = resolve(configPath, "..");
  if (config.env_file) loadEnvFile(resolve(base, config.env_file));
  if (config.task_model.provider !== "azure-openai-responses" || config.reference_model.provider !== "azure-openai-responses")
    throw new Error("This pool supports the azure-openai-responses provider for task and reference models");
  if (!process.env.AZURE_OPENAI_API_KEY) throw new Error("AZURE_OPENAI_API_KEY is required for the configured models");
  const models = createModels();
  models.setProvider(azureOpenAIResponsesProvider());
  const taskModel = models.getModel(config.task_model.provider, config.task_model.model);
  const referenceModel = models.getModel(config.reference_model.provider, config.reference_model.model);
  if (!taskModel || !referenceModel) throw new Error("Configured model is not in the installed Pi catalog");
  const maximum = {
    tokens: config.judgement_maximum?.tokens ?? 30_000,
    provider_calls: 1,
    cost_microunits: config.judgement_maximum?.cost_microunits ?? 30_000,
    sandbox_time_ms: 0,
    sandbox_cpu_ms: 0,
    output_bytes: 0,
  };
  const reference = new GenerativeProvider("azure-reference", models, referenceModel, maximum, 2048);
  let jev: JudgementProvider | undefined;
  if (config.jev) {
    if (!process.env.JEV_API_KEY) throw new Error("JEV_API_KEY is required when jev is configured");
    jev = new JevProvider("jev", config.jev.model, maximum, () => process.env.JEV_API_KEY!);
  }
  const processes = config.processes ?? SUPPORTED_PROCESSES;
  const classes = (process.env.MEMORY_POOL_CLASSES ?? "").split(",").filter(Boolean);
  const controller = new AbortController();
  for (const signal of ["SIGTERM", "SIGINT"] as const)
    process.on(signal, () => {
      console.log(`pool ${process.env.MEMORY_POOL_INDEX ?? ""}: ${signal} received; finishing current work`);
      controller.abort();
    });
  console.log(`pool ${process.env.MEMORY_POOL_INDEX ?? ""}: classes ${classes.join(",") || "?"}; processes ${processes.join(",")}; task ${taskModel.provider}/${taskModel.id}; reference ${referenceModel.id}; jev ${jev ? config.jev!.model : "off"}`);
  await runSupervisor(
    {
      host: new HostClient(hostUrl, token),
      hostUrl,
      processes,
      concurrency: config.concurrency ?? { interactive: 1, deferred: 1 },
      pollMs: config.poll_ms ?? 1000,
      leaseSeconds: config.lease_seconds ?? 120,
      sessionsDirectory: resolve(base, config.sessions_directory),
      models,
      taskModel,
      maxPayloadBytes: config.max_payload_bytes ?? 100_000,
      taskReservationCost: config.task_reservation_cost_microunits,
      judgement: { reference, jev, mode: config.judgement_mode ?? (jev ? "shadow" : "reference"), maximum },
      log: (line) => console.log(`pool ${process.env.MEMORY_POOL_INDEX ?? ""}: ${line}`),
    },
    withAbortSignal(controller.signal, BACKGROUND_CONTEXT),
  );
}

main().catch((error: unknown) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
});
