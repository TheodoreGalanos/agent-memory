// ABOUTME: Explicit pool composition: typed service installation, module declarations, and the validation
// ABOUTME: that rejects an incomplete or conflicting composition before any work is claimed.
import { createHash } from "node:crypto";
import type { Model, Api, Models } from "@earendil-works/pi-ai";
import type { Process } from "../../../contracts/generated/work-command.js";
import type { JudgementProvider } from "../../judgement/src/providers.js";
import type { JudgementOptions } from "../../judgement/src/runtime.js";
import type { SandboxBinding } from "../../pi-worker/src/asp-execution-env.js";
import { EXECUTION_TOOLS, toolName, type WorkerTool } from "../../pi-worker/src/harness.js";
import type { ProcessHandler } from "./runner.js";

/** Services a deployment installs once; modules declare which of them they require. */
export interface Services {
  taskModel?: {
    models: Models;
    model: Model<Api>;
    maxPayloadBytes: number;
    /** Cost reservation per task provider call, in microunits. Settles to observed usage. */
    reservationCost?: number;
  };
  judgement?: Pick<JudgementOptions, "primary" | "fallback" | "comparison"> & {
    maximum: JudgementProvider["maximum"];
  };
  /** Where a task's granted native tools actually run. Absent means tool-less: the brief's granted
   * `read|write|edit|bash|python` names go unused and only offered application tools reach the model. */
  execution?:
    | { kind: "node"; cwd?: string }
    | { kind: "asp"; binding: SandboxBinding; helperPath?: string };
}
export type Capability = keyof Services;

export interface ModuleDeclaration {
  id: string;
  version: string;
  /** Installed services this module cannot work without. */
  requires: Capability[];
  /** Installed services this module uses when present and works without otherwise. */
  optional?: Capability[];
  /** Processes this module handles; exactly one module per process. */
  operations: Process[];
}

/** A process module: its declaration plus the handlers (and optional tools) it attaches. */
export interface ProcessModule {
  declaration: ModuleDeclaration;
  /** Handlers for exactly the declared operations. Services are guaranteed to satisfy `requires`;
   * `offered` holds every application tool the composition's modules contribute. */
  attach(services: Services, offered: WorkerTool[]): ProcessHandler[];
  /** Application tools offered to Pi-driven operations; each brief decides which are granted. */
  tools?(services: Services): WorkerTool[];
}

export interface Composition {
  /** Human-readable identity with a content digest: `investigation@1+formation@1#0123abcd4567`. */
  identity: string;
  modules: ModuleDeclaration[];
  handlers: ProcessHandler[];
  tools: WorkerTool[];
}

/** Validates the modules against the installed services and returns the composed application. */
export function composeApplication(services: Services, modules: ProcessModule[]): Composition {
  if (!modules.length) throw new Error("A composition needs at least one module");
  const installed = (Object.keys(services) as Capability[]).filter((key) => services[key] != null).sort();
  const ids = new Set<string>();
  const owners = new Map<Process, string>();
  const tools: WorkerTool[] = [];
  const toolOwners = new Map<string, string>();
  // First pass: declarations, requirements and offered tools. Nothing is attached until all are valid.
  for (const module of modules) {
    const { id, version, requires, operations } = module.declaration;
    if (!id.trim() || !version.trim()) throw new Error("Every module needs an id and a version");
    if (ids.has(id)) throw new Error(`Module ${id} is declared more than once`);
    ids.add(id);
    const offered = module.tools?.(services) ?? [];
    if (!operations.length && !offered.length) throw new Error(`Module ${id} declares no operations`);
    const missing = requires.filter((capability) => !installed.includes(capability));
    if (missing.length) throw new Error(`Module ${id} requires ${missing.join(", ")} but the service is not installed`);
    for (const operation of operations) {
      const other = owners.get(operation);
      if (other) throw new Error(`Process ${operation} is handled by both ${other} and ${id}`);
      owners.set(operation, id);
    }
    for (const tool of offered) {
      const name = toolName(tool);
      if ((EXECUTION_TOOLS as readonly string[]).includes(name))
        throw new Error(`Module ${id} offers tool ${name}, which is a native execution tool`);
      const other = toolOwners.get(name);
      if (other) throw new Error(`Tool ${name} is offered by both ${other} and ${id}`);
      toolOwners.set(name, id);
      tools.push(tool);
    }
  }
  // Second pass: attach handlers with the full set of offered tools in hand.
  const handlers: ProcessHandler[] = [];
  for (const module of modules) {
    const { id, operations } = module.declaration;
    const attached = module.attach(services, tools);
    const declared = new Set(operations), provided = new Set(attached.map((h) => h.process));
    for (const handler of attached)
      if (!declared.has(handler.process)) throw new Error(`Module ${id} attached a ${handler.process} handler it did not declare`);
    for (const operation of operations)
      if (!provided.has(operation)) throw new Error(`Module ${id} declares ${operation} but attached no handler for it`);
    if (attached.length !== provided.size) throw new Error(`Module ${id} attached more than one handler for a process`);
    handlers.push(...attached);
  }
  const declarations = modules.map((m) => m.declaration);
  const digest = createHash("sha256")
    .update(JSON.stringify({ services: installed, modules: declarations, tools: [...toolOwners.keys()] }))
    .digest("hex")
    .slice(0, 12);
  const label = declarations.map((d) => `${d.id}@${d.version}`).join("+");
  return { identity: `${label}#${digest}`, modules: declarations, handlers, tools };
}
