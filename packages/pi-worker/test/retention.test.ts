import { mkdtemp, rm, copyFile, access } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { expect, it } from "vitest";
import { BACKGROUND_CONTEXT as context, value } from "@earendil-works/pi-agent-core";
import { openLocalSession, purgeLocalSession, applyLocalDeletionRegistry } from "../src/local-session.js";

it("purges the whole Pi container under exclusive ownership and rejects a restored old session", async () => {
  const root = await mkdtemp(join(tmpdir(), "memory-erasure-"));
  try {
    const sessions = join(root, "sessions");
    const old = await openLocalSession(sessions, "old");
    await old.session.setValue(value("private", "cached-context"), "deleted source text", context);
    await old.backup(join(root, "backup.sqlite"));
    await expect(purgeLocalSession(sessions, "old")).rejects.toThrow("already owned");
    await old.close();
    await purgeLocalSession(sessions, "old");
    for (const suffix of ["", "-wal", "-shm", ".worker.json"])
      await expect(access(join(sessions, `old.sqlite${suffix}`))).rejects.toThrow();
    await expect(openLocalSession(sessions, "old")).rejects.toThrow("deleted");
    // A restored data file cannot bypass the independently retained tombstone.
    await copyFile(join(root, "backup.sqlite"), join(sessions, "old.sqlite"));
    await expect(openLocalSession(sessions, "old")).rejects.toThrow("deleted");
    await applyLocalDeletionRegistry(sessions, ["old"]);
    const clean = await openLocalSession(sessions, "continuation");
    try {
      expect(await clean.session.getValue(value("private", "cached-context"), context)).toBeUndefined();
      expect(await clean.session.findEntries(undefined, context)).toEqual([]);
    } finally { await clean.close(); }
    await purgeLocalSession(sessions, "old");
  } finally { await rm(root, { recursive: true, force: true }); }
});

it("runs a fresh Pi continuation without replaying the deleted session's tool", async () => {
  const { createModels, fauxProvider, fauxAssistantMessage, fauxToolCall } = await import("@earendil-works/pi-ai");
  const { NodeExecutionEnv } = await import("@earendil-works/pi-agent-core/node");
  const { createWorkerHarness } = await import("../src/harness.js");
  const { readFile } = await import("node:fs/promises");
  const directory = await mkdtemp(join(tmpdir(), "memory-clean-continuation-"));
  const faux = fauxProvider();
  const models = createModels();
  models.setProvider(faux.provider);
  async function run(id: string) {
    const local = await openLocalSession(join(directory, "sessions"), id);
    const created = await createWorkerHarness({ session: local.session, models, model: faux.getModel(), env: new NodeExecutionEnv({ cwd: directory }), tools: ["bash"] }, context);
    try {
      const lane = await created.harness.lane("main", context);
      const accepted = await lane.accept({ kind: "prompt", operationId: id, prompt: "Perform the assigned work." }, context);
      expect(accepted.ok).toBe(true);
      expect(await lane.drive({ operationId: id }, context)).toMatchObject({ ok: true, value: { kind: "settled" } });
    } finally { await created.harness.close(context); await local.close(); }
  }
  try {
    faux.setResponses([
      fauxAssistantMessage(fauxToolCall("bash", { command: "printf performed >> effect.txt" }), { stopReason: "toolUse" }),
      fauxAssistantMessage("Old work settled."),
    ]);
    await run("old");
    await purgeLocalSession(join(directory, "sessions"), "old");
    faux.setResponses([transcript => {
      expect(JSON.stringify(transcript)).not.toContain("effect.txt");
      expect(transcript.messages.some(m => m.role === "toolResult")).toBe(false);
      return fauxAssistantMessage("Clean continuation settled.");
    }]);
    await run("clean");
    expect(await readFile(join(directory, "effect.txt"), "utf8")).toBe("performed");
  } finally { await rm(directory, { recursive: true, force: true }); }
});
