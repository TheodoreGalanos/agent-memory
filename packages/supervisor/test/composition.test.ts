// ABOUTME: Tests for explicit pool composition: module declarations are validated against installed
// ABOUTME: services before any work is claimed, and the composition identity is stable and content-derived.
import { expect, it } from "vitest";
import type { AgentHarnessTool, ExecutionToolContext } from "@earendil-works/pi-agent-core";
import { Type } from "@earendil-works/pi-ai";
import { composeApplication, type ProcessModule, type Services } from "../src/composition.js";
import type { ProcessHandler } from "../src/runner.js";
import { activationModule } from "../src/activation.js";
import { consolidationModule } from "../src/consolidation.js";
import { evaluationModule } from "../src/evaluation.js";
import { maintenanceModule } from "../src/maintenance.js";

const handler = (process: ProcessHandler["process"]): ProcessHandler => ({ process, async run() {} });
const tool = (name: string): AgentHarnessTool<ExecutionToolContext> => ({
  name, label: name, description: name, parameters: Type.Object({}), replay: "never",
  async execute() { return { content: [{ type: "text", text: "" }], details: null }; },
});
const module = (over: Partial<ProcessModule> & { id: string }): ProcessModule => ({
  declaration: { id: over.id, version: "1", requires: [], operations: ["investigation"] },
  attach: () => [handler("investigation")],
  ...over,
});
const judgementService = { judgement: {} } as unknown as Services;

it("accepts an installed execution service as satisfying an optional requirement", () => {
  const declaresExecution = module({
    id: "investigation",
    declaration: { id: "investigation", version: "1", requires: [], optional: ["execution"], operations: ["investigation"] },
  });
  const executionService = { execution: { kind: "node", cwd: "." } } as unknown as Services;
  expect(() => composeApplication(executionService, [declaresExecution])).not.toThrow();
  expect(() => composeApplication({}, [declaresExecution])).not.toThrow();
});

it("composes declared modules into handlers, contributed tools and a content-derived identity", () => {
  const formation = module({
    id: "formation",
    declaration: { id: "formation", version: "2", requires: ["judgement"], operations: ["formation"] },
    attach: () => [handler("formation")],
    tools: () => [tool("lookup")],
  });
  const composed = composeApplication(judgementService, [module({ id: "investigation" }), formation]);
  expect(composed.handlers.map((h) => h.process)).toEqual(["investigation", "formation"]);
  expect(composed.tools.map((t) => (typeof t === "string" ? t : t.name))).toEqual(["lookup"]);
  expect(composed.identity).toMatch(/^investigation@1\+formation@2#[0-9a-f]{12}$/);
  expect(composeApplication(judgementService, [module({ id: "investigation" }), formation]).identity).toBe(composed.identity);
  const bumped = { ...formation, declaration: { ...formation.declaration, version: "3" } };
  expect(composeApplication(judgementService, [module({ id: "investigation" }), bumped]).identity).not.toBe(composed.identity);
});

it("rejects a module whose required service is not installed", () => {
  const needsJudgement = module({ id: "formation", declaration: { id: "formation", version: "1", requires: ["judgement"], operations: ["formation"] }, attach: () => [handler("formation")] });
  expect(() => composeApplication({}, [needsJudgement])).toThrow(/formation.*judgement/);
});

it("rejects duplicate module ids, duplicate operations and handlers outside the declaration", () => {
  expect(() => composeApplication({}, [module({ id: "a" }), module({ id: "a" })])).toThrow(/a/);
  expect(() => composeApplication({}, [module({ id: "a" }), module({ id: "b" })])).toThrow(/investigation/);
  expect(() => composeApplication({}, [module({ id: "a", attach: () => [handler("formation")] })])).toThrow(/formation/);
  expect(() => composeApplication({}, [module({ id: "a", attach: () => [] })])).toThrow(/investigation/);
  expect(() => composeApplication({}, [])).toThrow(/module/i);
});

it("rejects a module that declares no operations and offers no tools, but accepts one that offers only tools", () => {
  const bare = module({ id: "a", declaration: { id: "a", version: "1", requires: [], operations: [] }, attach: () => [] });
  expect(() => composeApplication({}, [bare])).toThrow(/a.*no operations/);
  const toolOnly = module({ id: "a", declaration: { id: "a", version: "1", requires: [], operations: [] }, attach: () => [], tools: () => [tool("lookup")] });
  const composed = composeApplication({}, [toolOnly]);
  expect(composed.handlers).toEqual([]);
  expect(composed.tools.map((t) => (typeof t === "string" ? t : t.name))).toEqual(["lookup"]);
});

it("rejects contributed tools that collide with each other or with native execution tools", () => {
  const a = module({ id: "a", tools: () => [tool("lookup")] });
  const b = module({ id: "b", declaration: { id: "b", version: "1", requires: [], operations: ["formation"] }, attach: () => [handler("formation")], tools: () => [tool("lookup")] });
  expect(() => composeApplication({}, [a, b])).toThrow(/lookup/);
  expect(() => composeApplication({}, [module({ id: "a", tools: () => [tool("bash")] })])).toThrow(/bash/);
});

it("rejects consolidation, maintenance and activation without an installed judgement service", () => {
  expect(() => composeApplication({}, [consolidationModule])).toThrow(/consolidation.*judgement/);
  expect(() => composeApplication({}, [maintenanceModule])).toThrow(/maintenance.*judgement/);
  expect(() => composeApplication({}, [activationModule])).toThrow(/activation.*judgement/);
});

it("accepts consolidation, evaluation, maintenance and activation once their declared requirements are installed", () => {
  expect(() => composeApplication(judgementService, [consolidationModule])).not.toThrow();
  expect(() => composeApplication(judgementService, [maintenanceModule])).not.toThrow();
  expect(() => composeApplication(judgementService, [activationModule])).not.toThrow();
  // Evaluation declares no required service: its trials run through whichever optional service a candidate needs.
  expect(() => composeApplication({}, [evaluationModule])).not.toThrow();
  const composed = composeApplication(judgementService, [consolidationModule, maintenanceModule, activationModule, evaluationModule]);
  expect(composed.handlers.map((h) => h.process).sort()).toEqual(["activation", "consolidation", "evaluation", "maintenance"]);
});
