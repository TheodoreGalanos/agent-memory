import { randomUUID } from "node:crypto";
import { InterpreterClient } from "../src/interpreter-client.js";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { it, expect } from "vitest";
import { BACKGROUND_CONTEXT as context } from "@earendil-works/pi-agent-core";
import {
  createModels,
  fauxProvider,
  fauxAssistantMessage,
  fauxToolCall,
} from "@earendil-works/pi-ai";
import {
  AspExecutionEnv,
  type SandboxBinding,
} from "../src/asp-execution-env.js";
import { attachAssignedWorker } from "../src/assigned-worker.js";
import { openLocalSession } from "../src/local-session.js";
import { HostClient } from "../src/host-client.js";
import { SandboxClient } from "../src/sandbox-client.js";
import type {
  Assignment,
  WorkResult,
} from "../../../contracts/generated/assignment.js";

it.skipIf(!process.env.MEMORY_SANDBOX_FIXTURE)(
  "runs assigned native Pi tools in Harbor and publishes exports before completion",
  async () => {
    const fixture = JSON.parse(
      await readFile(process.env.MEMORY_SANDBOX_FIXTURE!, "utf8"),
    ) as {
      binding: SandboxBinding;
      assignment: Assignment;
      url: string;
      token: string;
      directory: string;
      bridge_config: string;
      allocation_id: string;
    };
    const env = await AspExecutionEnv.connect(fixture.binding, fixture.binding);
    const local = await openLocalSession(
      join(fixture.directory, "pi"),
      fixture.assignment.job.session_id,
    );
    const sandbox = new SandboxClient(
      join(process.cwd(), ".venv-harbor/bin/python"),
      fixture.bridge_config,
    );
    const host = new HostClient(fixture.url, fixture.token);
    const interpreter = new InterpreterClient(
      env,
      host,
      fixture.assignment,
      fixture.assignment.job.id,
    );
    const faux = fauxProvider();
    const models = createModels();
    models.setProvider(faux.provider);
    const result: WorkResult = {
      schema_version: "1",
      status: "complete",
      examined_scope: fixture.assignment.job.spec.brief.scope,
      inputs: fixture.assignment.job.spec.brief.inputs,
      findings: [],
      coverage: { examined: ["sandbox file"], unexamined: [] },
      unresolved_work: [],
      proposed_changes: [],
      child_outputs: [],
      known_effects: [],
      usage: { status: "unknown" },
    };
    faux.setResponses([
      fauxAssistantMessage(
        fauxToolCall("write", { path: "result.txt", content: "before\n" }),
        { stopReason: "toolUse" },
      ),
      fauxAssistantMessage(
        fauxToolCall("edit", {
          path: "result.txt",
          oldText: "before",
          newText: "after",
        }),
        { stopReason: "toolUse" },
      ),
      fauxAssistantMessage(fauxToolCall("read", { path: "result.txt" }), {
        stopReason: "toolUse",
      }),
      fauxAssistantMessage(
        fauxToolCall("bash", {
          command: "printf 'from bash\\n' >> result.txt",
        }),
        { stopReason: "toolUse" },
      ),
      fauxAssistantMessage(fauxToolCall("python", { action: "start" }), {
        stopReason: "toolUse",
      }),
      fauxAssistantMessage(
        fauxToolCall("python", {
          action: "execute",
          code: "print(sum(items))",
        }),
        { stopReason: "toolUse" },
      ),
      fauxAssistantMessage(JSON.stringify(result)),
    ]);
    const worker = await attachAssignedWorker(
      fixture.assignment,
      host,
      {
        ...fixture.assignment.job.spec.brief.profile,
        maxPayloadBytes: 64000,
        maxOutputTokens: faux.getModel().maxTokens,
        providerReservation: {
          tokens: 64000 + faux.getModel().maxTokens,
          provider_calls: 1,
          cost_microunits: 0,
          sandbox_cpu_ms: 0,
          sandbox_time_ms: 0,
          output_bytes: 0,
        },
      },
      {
        session: local.session,
        models,
        model: faux.getModel(),
        env,
        tools: ["read", "write", "edit", "bash", "python"],
        interpreter,
      },
      context,
      {
        async renew() {
          const binding = await sandbox.command<SandboxBinding>({
            action: "renew",
            identifier: fixture.allocation_id,
          });
          env.renewBinding(binding, binding);
        },
        async finish(cancelled) {
          await sandbox.command({
            action: "finish",
            identifier: fixture.allocation_id,
            allow_lost_exports: cancelled,
          });
        },
      },
    );
    try {
      // Assert the actual execution identity and secret boundary, not just container config.
      const updates: string[] = [];
      const identity = await env.exec(
        "id -u",
        {
          onUpdate: (u) => {
            if (u.kind === "replace") updates.push(u.output.text);
          },
        },
        context,
      );
      expect(identity).toMatchObject({ ok: true });
      expect(updates.join("").trim()).toBe("1000");
      expect(
        await env.readTextFile("/run/memory-asp/binding.json", context),
      ).toMatchObject({ ok: false, error: { code: "permission_denied" } });
      // A cloud egress proxy can acknowledge TCP before rejecting TLS/data.
      const outbound = await env.exec(
        'python3 -c \'import socket,ssl; s=socket.create_connection(("1.1.1.1",443),timeout=3); s=ssl.create_default_context().wrap_socket(s,server_hostname="one.one.one.one"); s.sendall(b"GET / HTTP/1.1\\r\\nHost: one.one.one.one\\r\\nConnection: close\\r\\n\\r\\n"); assert s.recv(1)\'',
        {},
        context,
      );
      expect(outbound).toMatchObject({ ok: true });
      if (outbound.ok) expect(outbound.value.exitCode).not.toBe(0);
      const blockedChange = await env.exec("iptables -F OUTPUT", {}, context);
      expect(blockedChange).toMatchObject({ ok: true });
      if (blockedChange.ok) expect(blockedChange.value.exitCode).not.toBe(0);
      await interpreter.start(context);
      expect(
        (
          await interpreter.execute(
            randomUUID(),
            "items = [1, 2, 3, 4]",
            context,
          )
        ).status,
      ).toBe("ok");
      const checkpoint = await interpreter.checkpoint(
        ["items"],
        randomUUID(),
        context,
      );
      await interpreter.stop(context);
      await interpreter.start(context);
      expect(
        (await interpreter.execute(randomUUID(), "print(items)", context))
          .status,
      ).toBe("failed");
      expect((await interpreter.restore(checkpoint, context)).status).toBe(
        "ok",
      );
      const settled = await worker.run();
      expect(settled).toMatchObject({
        kind: "settled",
        job: { state: "completed" },
      });
      expect(faux.state.callCount).toBe(7);
      const entries = await worker.lane.findEntries(
        { type: "message" },
        context,
      );
      const pythonOutput = entries.filter(
        (e) =>
          e.type === "message" &&
          e.message.role === "toolResult" &&
          e.message.toolName === "python",
      );
      expect(JSON.stringify(pythonOutput)).toContain("10\\n");
    } finally {
      await worker.close();
      await local.close();
      await env.cleanup(context);
    }
  },
  120_000,
);
