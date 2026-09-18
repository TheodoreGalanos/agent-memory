import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, rm, symlink, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { DatabaseSync } from "node:sqlite";
import { BACKGROUND_CONTEXT as context } from "@earendil-works/pi-agent-core";
import { expect, it } from "vitest";
import { openLocalSession } from "../src/local-session.js";

it("holds the canonical SQLite session lock across helper exit and releases it on close", async () => {
  const root = await mkdtemp(join(tmpdir(), "memory-owner-"));
  try {
    const directory = join(root, "sessions");
    const first = await openLocalSession(directory, "task:one");
    try {
      await symlink(directory, join(root, "alias"));
      await expect(
        openLocalSession(join(root, "alias"), "task:one"),
      ).rejects.toThrow("already owned");
      await first.session.setName("Retained after restart", context);
      const backup = join(root, "restore.sqlite");
      await first.backup(backup);
      const db = new DatabaseSync(backup, { readOnly: true });
      try {
        expect(db.prepare("PRAGMA integrity_check").get()).toMatchObject({
          integrity_check: "ok",
        });
      } finally {
        db.close();
      }
    } finally {
      await first.close();
    }
    const reopened = await openLocalSession(directory, "task:one");
    try {
      expect(await reopened.session.getName(context)).toBe(
        "Retained after restart",
      );
    } finally {
      await reopened.close();
    }
    const name = `~${Buffer.from("task:one", "utf16le").toString("base64url")}.sqlite.worker.json`;
    const manifest = JSON.parse(await readFile(join(directory, name), "utf8"));
    await writeFile(
      join(directory, name),
      JSON.stringify({ ...manifest, workerFamily: "incompatible-worker" }),
    );
    await expect(openLocalSession(directory, "task:one")).rejects.toThrow(
      "offline compatibility",
    );
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

it("releases the SQLite ownership lock when its worker process dies", async () => {
  const directory = await mkdtemp(join(tmpdir(), "memory-crash-"));
  const source = new URL("../src/local-session.ts", import.meta.url).href;
  const child = spawn(
    process.execPath,
    [
      "--input-type=module",
      "-e",
      `
    const { openLocalSession } = await import(${JSON.stringify(source)});
    const owner = await openLocalSession(process.argv[1], "crash-test");
    console.log("ready");
    setInterval(() => {}, 1000);
  `,
      directory,
    ],
    { stdio: ["ignore", "pipe", "pipe"] },
  );
  try {
    await new Promise<void>((resolve, reject) => {
      child.stdout.once("data", () => resolve());
      child.once("error", reject);
      child.once("exit", (code) =>
        reject(new Error(`Worker exited before attachment: ${code}`)),
      );
    });
    await expect(openLocalSession(directory, "crash-test")).rejects.toThrow(
      "already owned",
    );
    child.kill("SIGKILL");
    await once(child, "exit");
    const replacement = await openLocalSession(directory, "crash-test");
    await replacement.close();
  } finally {
    if (child.exitCode === null && child.signalCode === null) {
      child.kill("SIGKILL");
      await once(child, "exit");
    }
    await rm(directory, { recursive: true, force: true });
  }
});
