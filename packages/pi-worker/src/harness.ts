import { createInterpreterTool } from "./interpreter-tool.js";
import type { InterpreterClient } from "./interpreter-client.js";
import type { InvestigationPlan } from "./scoped-operation.js";
import { guardProviderCalls } from "./provider-guard.js";
import {
  AgentHarness,
  createBashTool,
  createEditTool,
  createReadTool,
  createWriteTool,
  type AgentHarnessOptions,
  type AgentHarnessTool,
  type Context,
  type ExecutionEnv,
  type ExecutionToolContext,
} from "@earendil-works/pi-agent-core";

import {
  attachWorkspace,
  workspaceProjector,
  resultProjector,
  type WorkspaceOptions,
} from "./workspace-harness.js";

export type ExecutionTool = "read" | "write" | "edit" | "bash" | "python";
export type WorkerOptions = Pick<
  AgentHarnessOptions<ExecutionToolContext>,
  "session" | "models" | "model"
> & {
  env: ExecutionEnv;
  workspace?: WorkspaceOptions;
  investigation?: InvestigationPlan;
  interpreter?: InterpreterClient;
  tools: ExecutionTool[];
  systemPrompt?: string;
  resources?: AgentHarnessOptions<ExecutionToolContext>["resources"];
  entryProjectors?: AgentHarnessOptions<ExecutionToolContext>["entryProjectors"];
  wrapTool?: (
    tool: AgentHarnessTool<ExecutionToolContext>,
  ) => AgentHarnessTool<ExecutionToolContext>;
};

/** Attach to a caller-owned session and execution environment. No host fallback. */
export async function createWorkerHarness(
  options: WorkerOptions,
  context: Context,
) {
  if (options.tools.includes("python") && !options.interpreter)
    throw new Error("Python requires an assigned sandbox interpreter client");
  const tools: Partial<
    Record<ExecutionTool, AgentHarnessTool<ExecutionToolContext>>
  > = {
    read: createReadTool(),
    write: createWriteTool(),
    edit: createEditTool(),
    bash: createBashTool(),
    ...(options.interpreter
      ? { python: createInterpreterTool(options.interpreter) }
      : {}),
  };
  const guard = guardProviderCalls(options.models);
  const created = await AgentHarness.create(
    {
      session: options.session,
      models: guard.models,
      model: options.model,
      toolContext: { env: options.env },
      tools: options.tools.map((name) =>
        options.wrapTool ? options.wrapTool(tools[name]!) : tools[name]!,
      ),
      resources: options.resources,
      entryProjectors: {
        ...options.entryProjectors,
        "memory.result": resultProjector,
        ...(options.workspace
          ? { "memory.workspace": workspaceProjector }
          : {}),
      },
      activeToolNames: options.tools,
      systemPrompt:
        options.systemPrompt ??
        "Work within the supplied scope. Keep observations and inferences distinct. State any unexamined evidence.",
    },
    context,
  );
  created.harness.events.on("handler_error", (event) => {
    if (
      event.kind === "hook" &&
      [
        "transform_context",
        "before_request",
        "before_payload",
        "before_compaction",
        "before_navigation",
      ].includes(event.hook)
    )
      guard.fail(event.error);
  });
  try {
    const workspace = options.workspace
      ? await attachWorkspace(
          created.harness,
          options.session,
          options.workspace,
          context,
        )
      : undefined;
    return { ...created, workspace };
  } catch (error) {
    await created.harness.close(context);
    throw error;
  }
}
