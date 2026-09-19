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
export const EXECUTION_TOOLS: readonly ExecutionTool[] = ["read", "write", "edit", "bash", "python"];
/** A native execution tool selected by name, or an application tool supplied by a module. */
export type WorkerTool = ExecutionTool | AgentHarnessTool<ExecutionToolContext>;
export const toolName = (tool: WorkerTool): string =>
  typeof tool === "string" ? tool : tool.name;
export type WorkerOptions = Pick<
  AgentHarnessOptions<ExecutionToolContext>,
  "session" | "models" | "model"
> & {
  env: ExecutionEnv;
  workspace?: WorkspaceOptions;
  investigation?: InvestigationPlan;
  interpreter?: InterpreterClient;
  /** Every tool, native or application, reaches the model through this one harness and the
   * same `wrapTool` boundary. Names are unique; application tools cannot shadow a native name. */
  tools: WorkerTool[];
  systemPrompt?: string;
  resources?: AgentHarnessOptions<ExecutionToolContext>["resources"];
  entryProjectors?: AgentHarnessOptions<ExecutionToolContext>["entryProjectors"];
  wrapTool?: (
    tool: AgentHarnessTool<ExecutionToolContext>,
  ) => AgentHarnessTool<ExecutionToolContext>;
};

/** Resolves the selected tools: native names to their implementations, application tools as given. */
export function resolveTools(
  options: Pick<WorkerOptions, "tools" | "interpreter">,
): AgentHarnessTool<ExecutionToolContext>[] {
  const native: Partial<
    Record<ExecutionTool, () => AgentHarnessTool<ExecutionToolContext>>
  > = {
    read: createReadTool,
    write: createWriteTool,
    edit: createEditTool,
    bash: createBashTool,
    ...(options.interpreter
      ? { python: () => createInterpreterTool(options.interpreter!) }
      : {}),
  };
  const seen = new Set<string>();
  return options.tools.map((tool) => {
    const name = toolName(tool);
    if (seen.has(name)) throw new Error(`Tool ${name} is selected more than once`);
    seen.add(name);
    if (typeof tool !== "string") {
      if ((EXECUTION_TOOLS as readonly string[]).includes(name))
        throw new Error(`Application tool ${name} would shadow a native execution tool`);
      return tool;
    }
    if (tool === "python" && !options.interpreter)
      throw new Error("Python requires an assigned sandbox interpreter client");
    return native[tool]!();
  });
}

/** Attach to a caller-owned session and execution environment. No host fallback. */
export async function createWorkerHarness(
  options: WorkerOptions,
  context: Context,
) {
  const tools = resolveTools(options);
  const guard = guardProviderCalls(options.models);
  const created = await AgentHarness.create(
    {
      session: options.session,
      models: guard.models,
      model: options.model,
      toolContext: { env: options.env },
      tools: tools.map((tool) =>
        options.wrapTool ? options.wrapTool(tool) : tool,
      ),
      resources: options.resources,
      entryProjectors: {
        ...options.entryProjectors,
        "memory.result": resultProjector,
        ...(options.workspace
          ? { "memory.workspace": workspaceProjector }
          : {}),
      },
      activeToolNames: tools.map((tool) => tool.name),
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
