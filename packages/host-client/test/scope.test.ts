// ABOUTME: Tests for the scope predicate shared by Pi workspace access checks and judgement disclosure:
// ABOUTME: a grant permits a request only when every constrained dimension is satisfied.
import { expect, it } from "vitest";
import type { Scope } from "../../../contracts/generated/workspace-state.js";
import { permits, readable } from "../src/scope.js";

const scope = (over: Partial<Scope> = {}): Scope => ({
  user_id: null, project_id: null, task_id: null, entity_ids: [], source_versions: [], ...over,
});

it("an unconstrained grant permits any request", () => {
  expect(permits(scope(), scope({ user_id: "u", project_id: "p", task_id: "t", entity_ids: ["e"], source_versions: [{ source_id: "s", revision: "1" }] }))).toBe(true);
});

it("identifier dimensions must match exactly when the grant names them", () => {
  const grant = scope({ project_id: "p" });
  expect(permits(grant, scope({ project_id: "p" }))).toBe(true);
  expect(permits(grant, scope({ project_id: "other" }))).toBe(false);
  // A request that leaves the constrained dimension open is not within the grant.
  expect(permits(grant, scope())).toBe(false);
});

it("entity and source constraints require a non-empty request contained in the grant", () => {
  const grant = scope({ entity_ids: ["a", "b"], source_versions: [{ source_id: "s", revision: "2" }] });
  expect(permits(grant, scope({ entity_ids: ["a"], source_versions: [{ source_id: "s", revision: "2" }] }))).toBe(true);
  expect(permits(grant, scope({ entity_ids: [], source_versions: [{ source_id: "s", revision: "2" }] }))).toBe(false);
  expect(permits(grant, scope({ entity_ids: ["a", "c"], source_versions: [{ source_id: "s", revision: "2" }] }))).toBe(false);
  // The same source at another revision is a different source version.
  expect(permits(grant, scope({ entity_ids: ["a"], source_versions: [{ source_id: "s", revision: "1" }] }))).toBe(false);
});

it("a task may read resources left open at project level, but not another task's or project's", () => {
  const task = scope({ project_id: "p", task_id: "t" });
  expect(readable(task, scope({ project_id: "p" }))).toBe(true);
  expect(readable(task, scope({ project_id: "p", task_id: "t" }))).toBe(true);
  expect(readable(task, scope({ project_id: "p", task_id: "other" }))).toBe(false);
  expect(readable(task, scope({ project_id: "q" }))).toBe(false);
  // Writing stays strict: a project-level record is not within a task's grant.
  expect(permits(task, scope({ project_id: "p" }))).toBe(false);
});
