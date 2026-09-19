// ABOUTME: Tests for explicit pool composition: module declarations are validated against installed
// ABOUTME: services before any work is claimed, and the composition identity is stable and content-derived.
import { expect, it } from "vitest";
import type { AgentHarnessTool, ExecutionToolContext } from "@earendil-works/pi-agent-core";
import { Type } from "@earendil-works/pi-ai";
import { composeApplication, type ProcessModule, type Services } from "../src/composition.js";
import type { ProcessHandler } from "../src/runner.js";

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

it("rejects contributed tools that collide with each other or with native execution tools", () => {
  const a = module({ id: "a", tools: () => [tool("lookup")] });
  const b = module({ id: "b", declaration: { id: "b", version: "1", requires: [], operations: ["formation"] }, attach: () => [handler("formation")], tools: () => [tool("lookup")] });
  expect(() => composeApplication({}, [a, b])).toThrow(/lookup/);
  expect(() => composeApplication({}, [module({ id: "a", tools: () => [tool("bash")] })])).toThrow(/bash/);
});
