import { randomUUID } from "node:crypto";
import { spawn, execFileSync } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, mkdir, readFile, writeFile, rm } from "node:fs/promises";
import { join } from "node:path";
import { userInfo } from "node:os";
import { createServer } from "node:net";
import {
  BACKGROUND_CONTEXT as context,
  withCancel,
  type ShellOutputUpdate,
} from "@earendil-works/pi-agent-core";
import { it, expect } from "vitest";
import {
  AspExecutionEnv,
  type SandboxBinding,
} from "../src/asp-execution-env.js";

it("rejects descriptors that differ from the trusted allocation", async () => {
  const binding: SandboxBinding = {
    sandbox_id: "a",
    generation: "1",
    session_id: "s",
    epoch: 1,
    expires_at: Date.now() + 30000,
    descriptor: {
      version: "0",
      transport: "ssh",
      workspace: "/workspace",
      connection: {
        host: "localhost",
        user: "test",
        host_key: "ssh-ed25519 YWJj",
      },
    },
  };
  await expect(
    AspExecutionEnv.connect({ ...binding, epoch: 2 }, binding),
  ).rejects.toThrow("trusted allocation");
  await expect(
    AspExecutionEnv.connect(
      {
        ...binding,
        descriptor: { ...binding.descriptor, transport: "unknown" as "ssh" },
      },
      binding,
    ),
  ).rejects.toThrow();
});

it.skipIf(process.env.MEMORY_TEST_SSH !== "1")(
  "runs binary, path, bounded shell and cancellation operations through real OpenSSH",
  async () => {
    const directory = await mkdtemp(join("/tmp", "memory-ssh-"));
    const workspace = join(directory, "workspace");
    const control = join(directory, "control");
    await mkdir(workspace);
    await mkdir(control);
    const hostKey = join(directory, "host");
    const clientKey = join(directory, "client");
    for (const key of [hostKey, clientKey])
      execFileSync("ssh-keygen", ["-q", "-t", "ed25519", "-N", "", "-f", key]);
    const server = createServer();
    server.listen(0, "127.0.0.1");
    await once(server, "listening");
    const address = server.address();
    if (!address || typeof address === "string")
      throw new Error("No test port");
    const port = address.port;
    await new Promise<void>((resolve) => server.close(() => resolve()));
    const helper = join(process.cwd(), "services/harbor-bridge/asp_helper.py");
    const config = join(directory, "sshd_config");
    const user = userInfo();
    await writeFile(
      config,
      `Port ${port}\nListenAddress 127.0.0.1\nHostKey ${hostKey}\nPidFile ${directory}/sshd.pid\nAuthorizedKeysFile ${clientKey}.pub\nStrictModes no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nAllowUsers ${user.username}\nAllowTcpForwarding no\nX11Forwarding no\nLogLevel VERBOSE\nForceCommand env MEMORY_ASP_CONTROL=${control} python3 ${helper}\n`,
    );
    const owner = {
      sandbox_id: "local-protocol-test",
      generation: "one",
      session_id: "test",
      epoch: 1,
      expires_at: Date.now() + 120_000,
      workspace,
      exec_uid: user.uid,
      exec_gid: user.gid,
    };
    await writeFile(join(control, "binding.json"), JSON.stringify(owner));
    const binding: SandboxBinding = {
      ...owner,
      descriptor: {
        version: "0",
        transport: "ssh",
        workspace,
        connection: {
          host: "127.0.0.1",
          port,
          user: user.username,
          identity: { file: clientKey },
          host_key: (await readFile(`${hostKey}.pub`, "utf8"))
            .trim()
            .split(" ")
            .slice(0, 2)
            .join(" "),
        },
      },
    };
    const sshd = spawn("/usr/sbin/sshd", ["-D", "-e", "-f", config], {
      stdio: ["ignore", "ignore", "pipe"],
    });
    let env: AspExecutionEnv | undefined;
    try {
      await new Promise<void>((resolve, reject) => {
        let stderr = "";
        sshd.stderr.on("data", (chunk) => {
          stderr += chunk.toString();
          if (stderr.includes("Server listening")) resolve();
        });
        sshd.once("error", reject);
        sshd.once("exit", () =>
          reject(new Error(`Test sshd unavailable: ${stderr}`)),
        );
      });
      env = await AspExecutionEnv.connect(binding, binding, helper);
      expect(await env.absolutePath("~", context)).toEqual({
        ok: true,
        value: user.homedir,
      });
      expect(
        await env.absolutePath(`file://${workspace}/space%20name`, context),
      ).toEqual({ ok: true, value: `${workspace}/space name` });
      const binary = Buffer.from([0, 255, 1, 254]);
      expect(
        await env.writeFile("space ' quote.bin", binary, context),
      ).toMatchObject({ ok: true });
      expect(await env.readBinaryFile("space ' quote.bin", context)).toEqual({
        ok: true,
        value: binary,
      });
      expect(
        await env.renameFile("space ' quote.bin", "renamed", context),
      ).toMatchObject({ ok: true });
      expect(await env.fileInfo("renamed", context)).toMatchObject({
        ok: true,
        value: { kind: "file", size: 4 },
      });
      expect(await env.readTextFile("missing", context)).toMatchObject({
        ok: false,
        error: { code: "not_found" },
      });
      expect(await env.writeFile("lines", "α\nβ\nlast", context)).toMatchObject(
        { ok: true },
      );
      expect(
        await env.readTextLines("lines", { maxLines: 2 }, context),
      ).toEqual({ ok: true, value: ["α", "β"] });
      expect(await env.joinPath(["/a", "/b", "..", "c"], context)).toEqual({
        ok: true,
        value: "/a/c",
      });
      const updates: ShellOutputUpdate[] = [];
      const result = await env.exec(
        "python3 -c 'print(\"αβγ\\n\"*20000)'",
        {
          capture: { limits: { maxBytes: 1000, maxLines: 10 }, spill: true },
          onUpdate: (update) => updates.push(update),
        },
        context,
      );
      expect(result).toMatchObject({
        ok: true,
        value: { exitCode: 0, truncation: { truncated: true } },
      });
      expect(JSON.stringify(updates).length).toBeLessThan(3000);
      if (!result.ok || !result.value.spillPath)
        throw new Error("No readable spill output");
      const spill = await env.openTextLineReader(
        result.value.spillPath,
        context,
      );
      expect(spill.ok).toBe(true);
      if (spill.ok) {
        expect(await spill.value.readLine(context)).toMatchObject({
          ok: true,
          value: { text: "αβγ" },
        });
        await spill.value.close(context);
      }
      const sessionId = randomUUID();
      const started = await env.interpreter(
        { action: "start", session_id: sessionId },
        context,
      );
      const interpreterScope = {
        session_id: sessionId,
        generation: started.generation,
      };
      expect(
        await env.interpreter(
          {
            ...interpreterScope,
            action: "execute",
            request_id: randomUUID(),
            code: "items = [2, 3]",
          },
          context,
        ),
      ).toMatchObject({ status: "ok" });
      expect(
        await env.interpreter(
          {
            ...interpreterScope,
            action: "execute",
            request_id: randomUUID(),
            code: "print(sum(items))",
          },
          context,
        ),
      ).toMatchObject({ status: "ok", output: "5\n" });
      await env.interpreter({ ...interpreterScope, action: "stop" }, context);
      const aborted = withCancel(context);
      const execution = env.exec("sleep 30 & wait", {}, aborted.context);
      setTimeout(() => aborted.cancel(), 500);
      expect(await execution).toMatchObject({
        ok: false,
        error: { code: "aborted" },
      });
      const closingExecution = env.exec("sleep 30 & wait", {}, context);
      await new Promise((resolve) => setTimeout(resolve, 500));
      await env.cleanup(context);
      expect(await closingExecution).toMatchObject({
        ok: false,
        error: { code: "aborted" },
      });
      env = await AspExecutionEnv.connect(binding, binding, helper);
      await writeFile(
        join(control, "binding.json"),
        JSON.stringify({ ...owner, epoch: 2 }),
      );
      expect(await env.readTextFile("lines", context)).toMatchObject({
        ok: false,
        error: { code: "permission_denied" },
      });
    } finally {
      await env?.cleanup(context);
      if (sshd.exitCode === null) {
        sshd.kill();
        await once(sshd, "exit");
      }
      await rm(directory, { recursive: true, force: true });
    }
  },
  60_000,
);
