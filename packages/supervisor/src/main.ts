// ABOUTME: Process entry for a worker pool child started by the Host. Reads its pool identity from the
// ABOUTME: environment and worker.json, installs only the services the enabled processes need, and runs until SIGTERM.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { loadEnvFile } from "node:process";
import { BACKGROUND_CONTEXT, withAbortSignal } from "@earendil-works/pi-agent-core";
import { createModels } from "@earendil-works/pi-ai";
import { azureOpenAIResponsesProvider } from "@earendil-works/pi-ai/providers/azure-openai-responses";
import { HostClient } from "../../host-client/src/client.js";
import { GenerativeProvider, JevProvider, type JudgementProvider } from "../../judgement/src/providers.js";
import { credentialsRequired, migrateWorkerConfiguration, type JudgementRoute } from "./configuration.js";
import { POOL_MODULES, SUPPORTED_PROCESSES, composeApplication, runSupervisor, type Services, type SupportedProcess } from "./index.js";

async function main() {
  const configPath = process.argv[2];
  if (!configPath) throw new Error("Usage: supervisor WORKER_CONFIG.json (pool identity comes from the environment)");
  const hostUrl = process.env.MEMORY_HOST_URL, token = process.env.MEMORY_POOL_TOKEN;
  if (!hostUrl || !token) throw new Error("MEMORY_HOST_URL and MEMORY_POOL_TOKEN are required");
  const say = (line: string) => console.log(`pool ${process.env.MEMORY_POOL_INDEX ?? ""}: ${line}`);
  const { config, migrated, notes } = migrateWorkerConfiguration(JSON.parse(readFileSync(configPath, "utf8")) as Record<string, unknown>);
  if (migrated) for (const note of notes) say(`configuration: ${note} Update ${configPath} to the judgement block to silence this notice.`);
  const base = resolve(configPath, "..");
  if (config.env_file) loadEnvFile(resolve(base, config.env_file));
  const processes = config.processes ?? SUPPORTED_PROCESSES;
  const unsupported = processes.filter((p) => !SUPPORTED_PROCESSES.includes(p));
  if (unsupported.length) throw new Error(`This pool supports ${SUPPORTED_PROCESSES.join(", ")} only; not ${unsupported.join(", ")}`);
  const missing = credentialsRequired(config, processes).filter((name) => !process.env[name]);
  if (missing.length) throw new Error(`${missing.join(", ")} ${missing.length > 1 ? "are" : "is"} required for the configured routes`);
  const models = createModels();
  models.setProvider(azureOpenAIResponsesProvider());
  const generativeModel = (route: { provider: string; model: string }) => {
    if (route.provider !== "azure-openai-responses") throw new Error("This pool supports the azure-openai-responses provider for generative models");
    const model = models.getModel(route.provider, route.model);
    if (!model) throw new Error(`Model ${route.provider}/${route.model} is not in the installed Pi catalog`);
    return model;
  };
  const maximum = {
    tokens: config.judgement.maximum?.tokens ?? 30_000,
    provider_calls: 1,
    cost_microunits: config.judgement.maximum?.cost_microunits ?? 30_000,
    sandbox_time_ms: 0,
    sandbox_cpu_ms: 0,
    output_bytes: 0,
  };
  const provider = (route: JudgementRoute, id: string): JudgementProvider =>
    route.kind === "jev"
      ? new JevProvider(id, route.model, maximum, () => process.env.JEV_API_KEY!)
      : new GenerativeProvider(id, models, generativeModel(route), maximum, 2048);
  const services: Services = {};
  const wantsTaskModel = processes.includes("investigation") || processes.includes("consolidation") || processes.includes("evaluation");
  const wantsExecution = processes.includes("investigation") || processes.includes("evaluation");
  if (wantsTaskModel) {
    const taskModel = generativeModel(config.task_model);
    services.taskModel = { models, model: taskModel, maxPayloadBytes: config.max_payload_bytes ?? 100_000, reservationCost: config.task_reservation_cost_microunits };
  }
  if (wantsExecution && config.execution)
    services.execution = config.execution.kind === "node"
      ? { kind: "node", cwd: config.execution.cwd }
      : { kind: "asp", binding: config.execution.binding, helperPath: config.execution.helper_path };
  if (processes.some((p) => ["formation", "consolidation", "maintenance", "activation"].includes(p))) {
    const { primary, fallback, comparison } = config.judgement;
    services.judgement = {
      primary: provider(primary, `primary:${primary.kind}`),
      fallback: fallback ? provider(fallback, `fallback:${fallback.kind}`) : undefined,
      comparison: comparison ? { provider: provider(comparison, `comparison:${comparison.kind}`) } : undefined,
      maximum,
    };
  }
  // Composition is validated here, before the pool credential claims anything.
  const composition = composeApplication(
    services,
    POOL_MODULES.filter((module) => module.declaration.operations.some((op) => processes.includes(op as SupportedProcess))),
  );
  const classes = (process.env.MEMORY_POOL_CLASSES ?? "").split(",").filter(Boolean);
  const controller = new AbortController();
  for (const signal of ["SIGTERM", "SIGINT"] as const)
    process.on(signal, () => {
      say(`${signal} received; finishing current work`);
      controller.abort();
    });
  const route = (r?: JudgementRoute) => (r ? `${r.kind}/${r.model}` : "off");
  say(`classes ${classes.join(",") || "?"}; composition ${composition.identity}; task ${services.taskModel ? `${services.taskModel.model.provider}/${services.taskModel.model.id}` : "off"}; judgement primary ${route(config.judgement.primary)}, fallback ${route(config.judgement.fallback)}, comparison ${route(config.judgement.comparison)}`)
  await runSupervisor(
    {
      host: new HostClient(hostUrl, token),
      hostUrl,
      composition,
      concurrency: config.concurrency ?? { interactive: 1, deferred: 1 },
      pollMs: config.poll_ms ?? 1000,
      leaseSeconds: config.lease_seconds ?? 120,
      drainMs: config.drain_ms ?? 5_000,
      sessionsDirectory: resolve(base, config.sessions_directory),
      log: say,
    },
    withAbortSignal(controller.signal, BACKGROUND_CONTEXT),
  );
}

main().catch((error: unknown) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
});
