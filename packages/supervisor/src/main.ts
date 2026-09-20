// ABOUTME: Process entry for a worker pool child started by the Host. Reads its pool identity from the
// ABOUTME: environment and worker.json, installs only the services the enabled processes need, and runs until SIGTERM.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { loadEnvFile } from "node:process";
import { BACKGROUND_CONTEXT, withAbortSignal } from "@earendil-works/pi-agent-core";
import { createModels, fauxProvider, fauxAssistantMessage, fauxToolCall, type Models, type Model, type Api } from "@earendil-works/pi-ai";
import { azureOpenAIResponsesProvider } from "@earendil-works/pi-ai/providers/azure-openai-responses";
import type { Resources } from "../../../contracts/generated/host-request.js";
import { HostClient } from "../../host-client/src/client.js";
import { GenerativeProvider, JevProvider, type JudgementProvider } from "../../judgement/src/providers.js";
import { credentialsRequired, migrateWorkerConfiguration, type JudgementRoute } from "./configuration.js";
import { POOL_MODULES, SUPPORTED_PROCESSES, composeApplication, runSupervisor, type Services, type SupportedProcess } from "./index.js";

/** Test-only judgement route: answers every question from a flat `{"family.key": "choice"}` JSON
 * file, still through the real `GenerativeProvider` serialization path (only its backing model is a
 * scripted faux one), so a scripted run exercises exactly the same packet/answer wiring a real
 * generative route does. `main` refuses this route unless `MEMORY_TEST_SCRIPTED=1` is set.
 *
 * J30 (a formation span's role) is answered structurally rather than from the file: only a bounded
 * tool exchange (a call with its own result) can hold durable material, so every other span (a lone
 * message or result) holds none. This is span shape, not scenario content, so every scripted run
 * gets it uniformly instead of repeating the same rule in every answers file. */
function scriptedJudgementProvider(id: string, answersFile: string, maximum: Resources): JudgementProvider {
  const answers = JSON.parse(readFileSync(answersFile, "utf8")) as Record<string, string>;
  const models = createModels();
  const faux = fauxProvider();
  models.setProvider(faux.provider);
  const generative = new GenerativeProvider(id, models, faux.getModel(), maximum, 2048, faux.getModel().id);
  return {
    id: generative.id,
    model: generative.model,
    release: generative.release,
    maximum,
    distributions: false,
    async evaluate(packet, signal) {
      const reply = Object.fromEntries(
        packet.questions.flatMap((d) =>
          d.definition.questions.map((q) => {
            const key = `${d.definition.id}.${q.key}`;
            if (key === "J30.role") {
              const span = packet.evidence.find((e) => e.name === "span")?.content as { kind?: string } | undefined;
              return [key, { type: "choice", choice: span?.kind === "exchange" ? "knowledge" : "none" }];
            }
            if (!(key in answers)) throw new Error(`MEMORY_TEST_SCRIPTED answers file ${answersFile} has no entry for ${key}`);
            return [key, { type: "choice", choice: answers[key] }];
          }),
        ),
      );
      faux.setResponses([fauxAssistantMessage(JSON.stringify({ answers: reply }))]);
      return generative.evaluate(packet, signal);
    },
  };
}

/** Test-only task model: replies to task model calls in order from a JSON array of steps. A
 * `tool_call` step calls the named native tool; an optional `delay_ms` holds the reply back (useful
 * for a scenario that needs a window to observe a job mid-flight before it settles). Any other step
 * is a partial WorkResult override (`status`, `coverage`, `findings`, `unresolved_work`) applied over
 * the WorkResult template `taskSystemPrompt` embeds in its own request (so `examined_scope` and
 * `inputs` always match the actual brief, never a value this test guessed). The last step repeats
 * once exhausted. `main` refuses this route unless `MEMORY_TEST_SCRIPTED=1` is set. */
function fauxTaskModel(repliesFile: string): { models: Models; model: Model<Api> } {
  const steps = JSON.parse(readFileSync(repliesFile, "utf8")) as (
    | { tool_call: { name: string; arguments: Record<string, unknown> }; delay_ms?: number }
    | (Partial<{ status: string; coverage: { examined: string[]; unexamined: string[] }; findings: unknown[]; unresolved_work: string[] }> & { delay_ms?: number })
  )[];
  const models = createModels();
  const faux = fauxProvider();
  models.setProvider(faux.provider);
  const textOf = (content: string | { type: string; text?: string }[]) =>
    typeof content === "string" ? content : content.map((c) => c.text ?? "").join("");
  let index = 0;
  const reply = async (t: { messages: { content: string | { type: string; text?: string }[] }[] }) => {
    const step = steps[Math.min(index, steps.length - 1)] ?? {};
    index += 1;
    if (step.delay_ms) await new Promise((r) => setTimeout(r, step.delay_ms));
    if ("tool_call" in step) return fauxAssistantMessage(fauxToolCall(step.tool_call.name, step.tool_call.arguments), { stopReason: "toolUse" });
    const all = t.messages.map((m) => textOf(m.content)).join("\n");
    const lines = all.split("\n").filter(Boolean);
    const template = JSON.parse(lines[lines.length - 1] ?? "{}") as Record<string, unknown>;
    return fauxAssistantMessage(
      JSON.stringify({
        ...template,
        status: step.status ?? "complete",
        coverage: step.coverage ?? { examined: ["Answered"], unexamined: [] },
        findings: step.findings ?? [],
        unresolved_work: step.unresolved_work ?? [],
      }),
    );
  };
  faux.setResponses(Array.from({ length: 32 }, () => reply));
  return { models, model: faux.getModel() };
}

async function main() {
  const configPath = process.argv[2];
  if (!configPath) throw new Error("Usage: supervisor WORKER_CONFIG.json (pool identity comes from the environment)");
  const hostUrl = process.env.MEMORY_HOST_URL, token = process.env.MEMORY_POOL_TOKEN;
  if (!hostUrl || !token) throw new Error("MEMORY_HOST_URL and MEMORY_POOL_TOKEN are required");
  const scripted = process.env.MEMORY_TEST_SCRIPTED === "1";
  const say = (line: string) => console.log(`pool ${process.env.MEMORY_POOL_INDEX ?? ""}: ${line}`);
  const { config, migrated, notes } = migrateWorkerConfiguration(JSON.parse(readFileSync(configPath, "utf8")) as Record<string, unknown>);
  if (migrated) for (const note of notes) say(`configuration: ${note} Update ${configPath} to the judgement block to silence this notice.`);
  const base = resolve(configPath, "..");
  if (config.env_file) loadEnvFile(resolve(base, config.env_file));
  const processes = config.processes ?? SUPPORTED_PROCESSES;
  const unsupported = processes.filter((p) => !SUPPORTED_PROCESSES.includes(p));
  if (unsupported.length) throw new Error(`This pool supports ${SUPPORTED_PROCESSES.join(", ")} only; not ${unsupported.join(", ")}`);
  const routes = [config.judgement.primary, config.judgement.fallback, config.judgement.comparison].filter((r): r is JudgementRoute => !!r);
  if (!scripted && (routes.some((r) => r.kind === "scripted") || config.task_model.provider === "faux"))
    throw new Error("judgement.*.kind \"scripted\" and task_model.provider \"faux\" are test-only; set MEMORY_TEST_SCRIPTED=1 to use them");
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
      : route.kind === "scripted"
        ? scriptedJudgementProvider(id, resolve(base, route.answers_file), maximum)
        : new GenerativeProvider(id, models, generativeModel(route), maximum, 2048);
  const services: Services = {};
  const wantsTaskModel = processes.includes("investigation") || processes.includes("consolidation") || processes.includes("evaluation");
  const wantsExecution = processes.includes("investigation") || processes.includes("evaluation");
  if (wantsTaskModel) {
    const taskModelConfig = config.task_model;
    const { models: taskModels, model: taskModel } =
      "replies_file" in taskModelConfig ? fauxTaskModel(resolve(base, taskModelConfig.replies_file)) : { models, model: generativeModel(taskModelConfig) };
    services.taskModel = { models: taskModels, model: taskModel, maxPayloadBytes: config.max_payload_bytes ?? 100_000, reservationCost: config.task_reservation_cost_microunits };
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
    // Process modules follow the enabled processes; a module that only offers tools (no operations)
    // joins whenever the services it requires are installed.
    POOL_MODULES.filter((module) =>
      module.declaration.operations.length
        ? module.declaration.operations.some((op) => processes.includes(op as SupportedProcess))
        : module.declaration.requires.every((capability) => services[capability] != null),
    ),
  );
  const classes = (process.env.MEMORY_POOL_CLASSES ?? "").split(",").filter(Boolean);
  const controller = new AbortController();
  for (const signal of ["SIGTERM", "SIGINT"] as const)
    process.on(signal, () => {
      say(`${signal} received; finishing current work`);
      controller.abort();
    });
  const route = (r?: JudgementRoute) => (r ? `${r.kind}/${"model" in r ? r.model : r.answers_file}` : "off");
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
