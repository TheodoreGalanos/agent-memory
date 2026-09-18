import { randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";
import { Pool } from "pg";
import { beforeAll, afterAll, it, expect } from "vitest";
import {
  BACKGROUND_CONTEXT as context,
  value,
  type SessionRepo,
} from "@earendil-works/pi-agent-core";
import {
  createSessionRepoConformance,
  createSessionRepoStreamingForkConformance,
  createStorageConformance,
} from "@earendil-works/pi-agent-core/harness/session/testing";
import {
  PgSessionRepo,
  PgStorage,
  migratePiPostgres,
  type PgOwnership,
} from "../src/postgres-session.js";
import {
  createModels,
  fauxProvider,
  fauxAssistantMessage,
} from "@earendil-works/pi-ai";
import { NodeExecutionEnv } from "@earendil-works/pi-agent-core/node";
import { createWorkerHarness } from "../src/harness.js";

import { workspaceFromBrief, briefAccess } from "../src/workspace.js";
import { input } from "./fixtures.js";

const enabled = !!process.env.MEMORY_TEST_POSTGRES_URL;
const pool = new Pool({
  connectionString: process.env.MEMORY_TEST_POSTGRES_URL,
});
const repos: PgSessionRepo[] = [];
beforeAll(async () => {
  if (!enabled) return;
  // This runner follows the Rust migration tests in the same disposable cluster.
  await migratePiPostgres(pool);
});
afterAll(async () => {
  await Promise.all(repos.map((r) => r.close(context)));
  await pool.end();
});
async function factory() {
  const owner: PgOwnership = {
    tenantId: randomUUID(),
    jobId: randomUUID(),
    ownerId: randomUUID(),
    epoch: 1,
  };
  const repo = new PgSessionRepo(pool, owner);
  repos.push(repo);
  // Grants are issued in call order so that concurrent create/fork calls reach the
  // repository's synchronous id reservation in the order the caller made them.
  let grants: Promise<unknown> = Promise.resolve();
  function grant(id: string) {
    const step = grants.then(() =>
      pool.query(
        "INSERT INTO session_leases(tenant_id,session_id,job_id,owner_id,epoch,expires_at) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT DO NOTHING",
        [
          owner.tenantId,
          id,
          owner.jobId,
          owner.ownerId,
          owner.epoch,
          Date.now() + 60_000,
        ],
      ),
    );
    grants = step.catch(() => undefined);
    return step;
  }
  // The fixture supplies coordinator-issued leases before each repository operation.
  const authorized: SessionRepo = {
    async create(options, ctx) {
      const id = options.id ?? randomUUID();
      await grant(id);
      return repo.create({ ...options, id }, ctx);
    },
    open: repo.open.bind(repo),
    list: repo.list.bind(repo),
    delete: repo.delete.bind(repo),
    async fork(source, options, ctx) {
      const id = options.id ?? randomUUID();
      await grant(id);
      return repo.fork(source, { ...options, id }, ctx);
    },
  };
  return { repo, authorized, owner, grant };
}
for (const test of createSessionRepoConformance(
  async () => (await factory()).authorized,
))
  it.skipIf(!enabled)(
    `PostgreSQL session: ${test.group}: ${test.name}`,
    test.run,
  );
for (const test of createSessionRepoStreamingForkConformance(
  async () => (await factory()).authorized,
))
  it.skipIf(!enabled)(`PostgreSQL streaming fork: ${test.name}`, test.run);
for (const test of createStorageConformance(async () => {
  const f = await factory();
  const session = await f.authorized.create({}, context);
  await session.close(context);
  const storage = new PgStorage(pool, f.owner, session.metadata.id);
  return {
    storage,
    async [Symbol.asyncDispose]() {
      await storage.close(context);
      await f.repo.close(context);
    },
  };
}))
  it.skipIf(!enabled)(
    `PostgreSQL storage: ${test.group}: ${test.name}`,
    test.run,
  );

it.skipIf(!enabled)(
  "fences current-state, entry, list, fork and delete mutations after takeover",
  async () => {
    const f = await factory();
    const session = await f.authorized.create({}, context);
    await session.setName("Before takeover", context);
    await pool.query(
      "UPDATE session_leases SET epoch=epoch+1 WHERE tenant_id=$1",
      [f.owner.tenantId],
    );
    await expect(session.setName("stale", context)).rejects.toThrow(
      "ownership",
    );
    await expect(
      session.appendList(
        { kind: "list", namespace: "test", key: "q" },
        "stale",
        context,
      ),
    ).rejects.toThrow("ownership");
    await expect(
      f.authorized.fork(session.metadata, { scope: "tree" }, context),
    ).rejects.toThrow("ownership");
    await session.close(context);
    await expect(f.repo.delete(session.metadata, context)).rejects.toThrow(
      "ownership",
    );
    const replacement = new PgSessionRepo(pool, { ...f.owner, epoch: 2 });
    repos.push(replacement);
    const reopened = await replacement.open(session.metadata, context);
    expect(await reopened.getName(context)).toBe("Before takeover");
    await reopened.setName("Replacement", context);
    await reopened.close(context);
  },
);

it.skipIf(!enabled)(
  "persists real Pi admission and settlement across PostgreSQL reopen",
  async () => {
    const f = await factory();
    const session = await f.authorized.create({}, context);
    const faux = fauxProvider();
    const models = createModels();
    models.setProvider(faux.provider);
    faux.setResponses([fauxAssistantMessage("Persisted result")]);
    const options = {
      models,
      model: faux.getModel(),
      env: new NodeExecutionEnv({ cwd: process.cwd() }),
      tools: [],
    };
    const first = await createWorkerHarness({ ...options, session }, context);
    const lane = await first.harness.lane("main", context);
    const operationId = randomUUID();
    expect(
      (
        await lane.accept(
          { kind: "prompt", operationId, prompt: "Inspect fixture" },
          context,
        )
      ).ok,
    ).toBe(true);
    await first.harness.close(context);
    const reopened = await f.repo.open(session.metadata, context);
    const second = await createWorkerHarness(
      { ...options, session: reopened },
      context,
    );
    const resumed = await second.harness.lane("main", context);
    expect(await resumed.drive({ operationId }, context)).toMatchObject({
      ok: true,
      value: { kind: "settled", outcome: { status: "completed" } },
    });
    await second.harness.close(context);
    const third = await createWorkerHarness(
      { ...options, session: await f.repo.open(session.metadata, context) },
      context,
    );
    expect(
      await (
        await third.harness.lane("main", context)
      ).getResult(operationId, context),
    ).toMatchObject({ status: "completed" });
    expect(faux.state.callCount).toBe(1);
    await third.harness.close(context);
  },
);

it.skipIf(!enabled)(
  "restores the WP06 workspace and render manifest after PostgreSQL compaction",
  async () => {
    const f = await factory();
    const session = await f.authorized.create({}, context);
    const brief = input("before-correction").command.payload;
    const initial = workspaceFromBrief(
      brief,
      new Date(Date.now() + 60_000).toISOString(),
      new Date(Date.now() + 120_000).toISOString(),
    );
    const faux = fauxProvider();
    const models = createModels();
    models.setProvider(faux.provider);
    const options = {
      models,
      model: faux.getModel(),
      tools: [],
      env: new NodeExecutionEnv({ cwd: process.cwd() }),
      workspace: {
        initial,
        profile: brief.profile,
        maxPayloadBytes: 30_000,
        readAccess: async () => briefAccess(brief),
      },
    };
    const first = await createWorkerHarness({ ...options, session }, context);
    const lane = await first.harness.lane("main", context);
    faux.setResponses([fauxAssistantMessage("The source remains unexamined.")]);
    expect(await lane.prompt("Inspect", undefined, context)).toMatchObject({
      ok: true,
      value: { status: "completed" },
    });
    const manifest = await first.workspace!.latestManifest();
    await first.harness.setCompactionSettings(
      { enabled: true, reserveTokens: 1024, keepRecentTokens: 0 },
      context,
    );
    expect(await lane.compact(undefined, context)).toMatchObject({ ok: true });
    await first.harness.close(context);
    const second = await createWorkerHarness(
      { ...options, session: await f.repo.open(session.metadata, context) },
      context,
    );
    expect(await second.workspace!.state()).toEqual(initial);
    expect(await second.workspace!.latestManifest()).toEqual(manifest);
    faux.setResponses([
      fauxAssistantMessage("Continue under the original scope."),
    ]);
    expect(
      await (
        await second.harness.lane("main", context)
      ).prompt("Continue", undefined, context),
    ).toMatchObject({ ok: true, value: { status: "completed" } });
    expect(
      (await second.workspace!.latestManifest())!.selected.map((e) => e.id),
    ).toContain("governing-frame");
    await second.harness.close(context);
  },
);

it.skipIf(!enabled)("purges deleted Pi state and cannot reopen it even with a restored lease", async () => {
  const { purgePostgresSession } = await import("../src/postgres-session.js");
  const { repo, owner, grant } = await factory();
  const id = randomUUID();
  await grant(id);
  const session = await repo.create({ id }, context);
  await session.setValue(value("private", "packet"), "deleted text", context);
  await expect(purgePostgresSession(pool, owner.tenantId, id)).rejects.toThrow("revocation");
  await pool.query("INSERT INTO deleted_sessions(tenant_id,session_id) VALUES($1,$2)", [owner.tenantId, id]);
  await expect(session.getValue(value("private", "packet"), context)).rejects.toThrow("deleted");
  await session.close(context);
  await purgePostgresSession(pool, owner.tenantId, id);
  const rows = await pool.query("SELECT * FROM pi_sessions.values WHERE tenant_id=$1 AND session_id=$2", [owner.tenantId, id]);
  expect(rows.rowCount).toBe(0);
  await expect(repo.create({ id }, context)).rejects.toThrow("deleted");
});
