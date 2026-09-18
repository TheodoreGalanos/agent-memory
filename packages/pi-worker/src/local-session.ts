import { spawn } from "node:child_process";
import { mkdir, realpath, readFile, writeFile, open, access, rm } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { DatabaseSync, backup } from "node:sqlite";
import { BACKGROUND_CONTEXT as context } from "@earendil-works/pi-agent-core";
import {
  createNodeSqliteFactory,
  SqliteSessionRepo,
  SQLITE_STORAGE_VERSION,
} from "@earendil-works/pi-session-backend-sqlite-node";

const workerFamily = "pi-0.85.1";
const lockScript = fileURLToPath(
  new URL("../../../scripts/session-lock.py", import.meta.url),
);

async function acquireLock(path: string) {
  const handle = await open(path, "a", 0o600);
  try {
    const child = spawn("python3", [lockScript], {
      stdio: ["ignore", "ignore", "pipe", handle.fd],
    });
    const code = await new Promise<number | null>((resolve, reject) => {
      child.once("error", reject);
      child.once("exit", resolve);
    });
    if (code !== 0)
      throw new Error("Session is already owned or its OS lock is unavailable");
    return {
      assertHeld() {
        if (handle.fd < 0) throw new Error("Session OS lock is closed");
      },
      async close() {
        await handle.close();
      },
    };
  } catch (error) {
    await handle.close();
    throw error;
  }
}

/** The kernel lock covers the canonical directory and the pinned backend's file name. */
export async function openLocalSession(directory: string, id: string) {
  await mkdir(directory, { recursive: true, mode: 0o700 });
  const canonical = await realpath(directory);
  const databasePath = localSessionPath(canonical, id);
  const lock = await acquireLock(`${databasePath}.owner`);
  const base = createNodeSqliteFactory();
  const factory = {
    ...base,
    async open(path: string) {
      const db = await base.open(path);
      return guard(db);
    },
    async openExisting(path: string) {
      const db = await base.openExisting(path);
      return guard(db);
    },
  };
  function guard(db: Awaited<ReturnType<typeof base.open>>) {
    return {
      ...db,
      exec: db.exec.bind(db),
      prepare: db.prepare.bind(db),
      close: db.close.bind(db),
      transaction<T>(callback: () => T): T {
        lock.assertHeld();
        return db.transaction(() => {
          lock.assertHeld();
          return callback();
        });
      },
    };
  }
  const repo = new SqliteSessionRepo({
    directory: canonical,
    databaseFactory: factory,
  });
  try {
    try {
      await access(`${databasePath}.deleted`);
      throw new Error("Session was deleted; a new clean session is required");
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
    }
    const catalogue = await repo.list(undefined, context);
    const metadata = catalogue.find((item) => item.id === id);
    const manifestPath = `${databasePath}.worker.json`;
    if (metadata) {
      const manifest: unknown = JSON.parse(
        await readFile(manifestPath, "utf8"),
      );
      if (
        !manifest ||
        typeof manifest !== "object" ||
        !("workerFamily" in manifest) ||
        manifest.workerFamily !== workerFamily ||
        metadata.storageVersion !== SQLITE_STORAGE_VERSION
      ) {
        throw new Error(
          "Session needs an offline compatibility review before this worker can open it",
        );
      }
    } else {
      await writeFile(
        manifestPath,
        JSON.stringify({
          workerFamily,
          sqliteStorageVersion: SQLITE_STORAGE_VERSION,
        }),
        { mode: 0o600 },
      );
    }
    const session = metadata
      ? await repo.open(metadata, context)
      : await repo.create({ id }, context);
    let closed = false;
    return {
      session,
      async close() {
        if (closed) return;
        closed = true;
        try {
          await repo.close(context);
        } finally {
          await lock.close();
        }
      },
      async backup(destination: string) {
        lock.assertHeld();
        const source = new DatabaseSync(databasePath, { readOnly: true });
        try {
          await backup(source, destination);
        } finally {
          source.close();
        }
      },
    };
  } catch (error) {
    try {
      await repo.close(context);
    } finally {
      await lock.close();
    }
    throw error;
  }
}

/** The marker is kept outside the session container and must accompany a restore. */
export async function purgeLocalSession(directory: string, id: string) {
  await mkdir(directory, { recursive: true, mode: 0o700 });
  const canonical = await realpath(directory);
  const databasePath = localSessionPath(canonical, id);
  const lock = await acquireLock(`${databasePath}.owner`);
  const repo = new SqliteSessionRepo({ directory: canonical, databaseFactory: createNodeSqliteFactory() });
  try {
    await writeFile(`${databasePath}.deleted`, "revoked\n", { mode: 0o600 });
    const metadata = (await repo.list(undefined, context)).find(item => item.id === id);
    if (metadata) await repo.delete(metadata, context);
    // The upstream backend removes SQLite, WAL and SHM. Verify the boundary before
    // acknowledging it; the worker manifest is our own remaining sidecar.
    for (const suffix of ["", "-wal", "-shm"]) {
      try { await access(`${databasePath}${suffix}`); }
      catch (error) { if ((error as NodeJS.ErrnoException).code === "ENOENT") continue; throw error; }
      throw new Error("Pi backend left session storage behind");
    }
    await rm(`${databasePath}.worker.json`, { force: true });
  } finally {
    try { await repo.close(context); } finally { await lock.close(); }
  }
}

function localSessionPath(directory: string, id: string) {
  const name = /^[A-Za-z0-9_-]+$/.test(id) ? id : `~${Buffer.from(id, "utf16le").toString("base64url")}`;
  return join(directory, `${name}.sqlite`);
}

/** Call while the restored directory is still offline, using the separately retained registry. */
export async function applyLocalDeletionRegistry(directory: string, sessions: readonly string[]) {
  for (const id of sessions) await purgeLocalSession(directory, id);
}
