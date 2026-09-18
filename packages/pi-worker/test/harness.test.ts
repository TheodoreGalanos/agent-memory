import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { BACKGROUND_CONTEXT as context } from "@earendil-works/pi-agent-core";
import { NodeExecutionEnv } from "@earendil-works/pi-agent-core/node";
import {
  createModels,
  fauxAssistantMessage,
  fauxProvider,
  fauxToolCall,
} from "@earendil-works/pi-ai";
import {
  createNodeSqliteFactory,
  SqliteSessionRepo,
} from "@earendil-works/pi-session-backend-sqlite-node";
import { describe, expect, it } from "vitest";
import { createWorkerHarness } from "../src/harness.js";
import { checkpoints, input, scriptedResult } from "./fixtures.js";

async function fixture(tools: ("read" | "bash")[] = ["read"]) {
  const directory = await mkdtemp(join(tmpdir(), "memory-harness-"));
  const repo = new SqliteSessionRepo({
    directory: join(directory, "sessions"),
    databaseFactory: createNodeSqliteFactory(),
  });
  const session = await repo.create({}, context);
  const faux = fauxProvider();
  const models = createModels();
  models.setProvider(faux.provider);
  const options = {
    session,
    models,
    model: faux.getModel(),
    env: new NodeExecutionEnv({ cwd: directory }),
    tools,
  };
  const created = await createWorkerHarness(options, context);
  // The evaluator owns the other files. The checkpoint worker can read only
  // the evidence copy placed in this test's execution directory.
  created.harness.hooks.on("before_tool", ({ toolName, args }) => {
    if (
      toolName === "read" &&
      (typeof args.path !== "string" ||
        resolve(directory, args.path) !== join(directory, "evidence.json"))
    ) {
      return {
        block: { reason: "Outside checkpoint evidence", terminate: true },
      };
    }
    return undefined;
  });
  return {
    ...created,
    directory,
    repo,
    session,
    faux,
    options,
    async cleanup() {
      await created.harness.close(context);
      await repo.close(context);
      await rm(directory, { recursive: true, force: true });
    },
  };
}

describe("compiled Pi harness with a scripted provider (T03)", () => {
  it.each(checkpoints)(
    "runs %s, then reopens its SQLite session",
    async (name) => {
      const f = await fixture();
      try {
        const visible = input(name);
        await writeFile(
          join(f.directory, "evidence.json"),
          JSON.stringify(visible.evidence),
        );
        const expected = scriptedResult(name);
        f.faux.setResponses([
          fauxAssistantMessage(
            fauxToolCall("read", { path: "evidence.json" }),
            { stopReason: "toolUse" },
          ),
          (transcript) => {
            const tool = transcript.messages.find(
              (m) => m.role === "toolResult",
            );
            expect(tool).toMatchObject({ role: "toolResult", isError: false });
            expect(JSON.stringify(transcript)).not.toContain("90 min");
            expect(JSON.stringify(transcript)).not.toContain("reference-world");
            return fauxAssistantMessage(JSON.stringify(expected));
          },
        ]);
        const lane = await f.harness.lane("main", context);
        const operationId = visible.command.request_id;
        const admission = await lane.accept(
          {
            kind: "prompt",
            operationId,
            prompt: JSON.stringify(visible.command.payload),
          },
          context,
        );
        expect(admission.ok).toBe(true);
        expect((await lane.inspectExecution(context)).current?.id).toBe(
          operationId,
        );
        expect(await lane.drive({ operationId }, context)).toMatchObject({
          ok: true,
          value: { kind: "settled", outcome: { status: "completed" } },
        });
        const entries = await lane.findEntries(
          { order: "newestFirst" },
          context,
        );
        const answer = entries.find(
          (entry) =>
            entry.type === "message" && entry.message.role === "assistant",
        );
        if (answer?.type !== "message" || answer.message.role !== "assistant")
          throw new Error("No assistant result");
        const text = answer.message.content.find(
          (part) => part.type === "text",
        );
        if (text?.type !== "text") throw new Error("No result text");
        expect(JSON.parse(text.text).status).toBe(expected.status);
        // A completed Pi operation can return blocked/partial application work.
        expect((await lane.getResult(operationId, context))?.status).toBe(
          "completed",
        );
        await f.harness.close(context);
        const reopened = await f.repo.open(f.session.metadata, context);
        const resumed = await createWorkerHarness(
          { ...f.options, session: reopened },
          context,
        );
        try {
          expect(resumed.open).toHaveLength(0);
          const resumedLane = await resumed.harness.lane("main", context);
          expect(
            (await resumedLane.getResult(operationId, context))?.status,
          ).toBe("completed");
          expect(f.faux.state.callCount).toBe(2);
        } finally {
          await resumed.harness.close(context);
        }
      } finally {
        await f.cleanup();
      }
    },
  );

  it("executes the selected bash tool through the supplied environment", async () => {
    const f = await fixture(["bash"]);
    try {
      f.faux.setResponses([
        fauxAssistantMessage(
          fauxToolCall("bash", { command: "printf 'tool-boundary-ok'" }),
          { stopReason: "toolUse" },
        ),
        (transcript) => {
          const tool = transcript.messages.find((m) => m.role === "toolResult");
          expect(JSON.stringify(tool)).toContain("tool-boundary-ok");
          return fauxAssistantMessage("Command inspected.");
        },
      ]);
      const lane = await f.harness.lane("main", context);
      expect(
        await lane.prompt("Run the fixture command.", undefined, context),
      ).toMatchObject({ ok: true, value: { status: "completed" } });
    } finally {
      await f.cleanup();
    }
  });

  it("can abort admitted work before a provider call", async () => {
    const f = await fixture();
    try {
      const lane = await f.harness.lane("main", context);
      const operationId = input("before-correction").command.request_id;
      expect(
        (
          await lane.accept(
            { kind: "prompt", operationId, prompt: "Inspect C" },
            context,
          )
        ).ok,
      ).toBe(true);
      expect((await lane.requestAbort(operationId, context)).ok).toBe(true);
      expect(await lane.drive({ operationId }, context)).toMatchObject({
        ok: true,
        value: { kind: "settled", outcome: { status: "aborted" } },
      });
      expect(f.faux.state.callCount).toBe(0);
    } finally {
      await f.cleanup();
    }
  });

  it("blocks access to evaluator facts outside the checkpoint", async () => {
    const f = await fixture();
    try {
      const protectedFile = fileURLToPath(
        new URL(
          "../../../evals/property-location/reference-world.json",
          import.meta.url,
        ),
      );
      f.faux.setResponses([
        fauxAssistantMessage(fauxToolCall("read", { path: protectedFile }), {
          stopReason: "toolUse",
        }),
      ]);
      const lane = await f.harness.lane("main", context);
      expect(await lane.getActiveTools(context)).toEqual(["read"]);
      await lane.prompt("Inspect the available evidence.", undefined, context);
      const entries = await lane.findEntries({ type: "message" }, context);
      const tool = entries.find(
        (entry) =>
          entry.type === "message" && entry.message.role === "toolResult",
      );
      expect(tool).toMatchObject({
        type: "message",
        message: { role: "toolResult", isError: true },
      });
      expect(JSON.stringify(tool)).toContain("Outside checkpoint evidence");
      expect(JSON.stringify(tool)).not.toContain("90 min");
      expect(f.faux.state.callCount).toBe(1);
    } finally {
      await f.cleanup();
    }
  });

  it("reports an unavailable model as failed work", async () => {
    const f = await fixture();
    try {
      const lane = await f.harness.lane("main", context);
      await lane.setModel(
        { provider: "not-registered", modelId: "missing" },
        context,
      );
      expect(await lane.prompt("Inspect C", undefined, context)).toMatchObject({
        ok: true,
        value: { status: "failed", error: { code: "model_unavailable" } },
      });
      expect(f.faux.state.callCount).toBe(0);
    } finally {
      await f.cleanup();
    }
  });
});
