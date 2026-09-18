import { spawnSync } from "node:child_process";
import { Ajv2020 } from "ajv/dist/2020.js";
import { fullFormats } from "ajv-formats/dist/formats.js";
import { loadJson } from "./fixtures.js";
import { randomUUID } from "node:crypto";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  BACKGROUND_CONTEXT as context,
  type AgentMessage,
} from "@earendil-works/pi-agent-core";
import { NodeExecutionEnv } from "@earendil-works/pi-agent-core/node";
import {
  createModels,
  fauxAssistantMessage,
  fauxProvider,
  fauxToolCall,
  createAssistantMessageEventStream,
  type Usage,
} from "@earendil-works/pi-ai";
import {
  createNodeSqliteFactory,
  SqliteSessionRepo,
} from "@earendil-works/pi-session-backend-sqlite-node";
import { describe, expect, it } from "vitest";
import { createWorkerHarness } from "../src/harness.js";
import {
  workspaceFromBrief,
  formationInputs,
  reconcileWorkspace,
  emptyInputs,
  validateWorkspace,
  type WorkspaceEntry,
  type WorkspaceAccess,
} from "../src/workspace.js";
import {
  validateToolPairs,
  type WorkspaceOptions,
} from "../src/workspace-harness.js";
import { input, scriptedResult } from "./fixtures.js";

function workingState() {
  const brief = input("before-correction").command.payload;
  const state = workspaceFromBrief(
    brief,
    new Date(Date.now() + 600_000).toISOString(),
    new Date(Date.now() + 900_000).toISOString(),
  );
  const entry = (
    id: string,
    text: string,
    kind: WorkspaceEntry["kind"] = "observation",
  ): WorkspaceEntry => ({
    ...structuredClone(state.entries[0]),
    id,
    text,
    kind,
    decision_relevant: false,
    evidential_status: "observation",
    inputs: emptyInputs(),
  });
  state.entries.push(
    entry("position-a", "Location C has an incomplete fire resistance value."),
    entry(
      "position-b",
      "The type supplies 60 minutes, but its use here is disputed.",
    ),
    {
      ...entry(
        "obligation",
        "Resolve the C instance/type conflict before a final answer.",
        "obligation",
      ),
      decision_relevant: true,
    },
    {
      ...entry(
        "child",
        "Child inspected the assigned source only.",
        "hypothesis",
      ),
      origin: "agent_generated",
      evidential_status: "inference",
      exposure: "delegated_finding",
      generating_operation: randomUUID(),
      supporting_entries: ["position-a"],
    },
  );
  state.conflicts.push({
    id: "c-conflict",
    members: ["position-a", "position-b"],
    status: "Unresolved instance/type disagreement",
    unresolved: true,
  });
  return { state, entry, brief };
}

async function fixture(maxPayloadBytes = 40_000) {
  const directory = await mkdtemp(join(tmpdir(), "memory-workspace-"));
  const repo = new SqliteSessionRepo({
    directory: join(directory, "sessions"),
    databaseFactory: createNodeSqliteFactory(),
  });
  const session = await repo.create({}, context);
  const faux = fauxProvider();
  const models = createModels();
  let access: WorkspaceAccess = { revision: "1", allows: () => true };
  let payloadPadding = "";
  let reportedUsage: Usage | undefined;
  let beforePayload: (() => void) | undefined;
  const payloads: unknown[] = [];
  // Faux itself has no transport payload hook. This test transport exercises the
  // same onPayload boundary as Pi's network providers, before delivering a reply.
  models.setProvider({
    ...faux.provider,
    streamSimple(model, transcript, options) {
      const outer = createAssistantMessageEventStream();
      queueMicrotask(async () => {
        try {
          beforePayload?.();
          const payload = { ...transcript, padding: payloadPadding };
          await options?.onPayload?.(payload, model);
          payloads.push(payload);
          const stream = faux.provider.streamSimple(model, transcript, options);
          for await (const event of stream) {
            if (event.type === "done" && reportedUsage)
              event.message.usage = reportedUsage;
            outer.push(event);
          }
          const result = await stream.result();
          if (reportedUsage) result.usage = reportedUsage;
          outer.end(result);
        } catch (error) {
          const message = fauxAssistantMessage("", {
            stopReason: "error",
            errorMessage: String(error),
          });
          outer.push({ type: "error", reason: "error", error: message });
          outer.end(message);
        }
      });
      return outer;
    },
  });
  const { state, entry, brief } = workingState();
  const workspace: WorkspaceOptions = {
    initial: state,
    profile: brief.profile,
    maxPayloadBytes,
    reservedResultTokens: 512,
    readAccess: async () => access,
  };
  const options = {
    session,
    models,
    model: faux.getModel(),
    env: new NodeExecutionEnv({ cwd: directory }),
    tools: [],
    workspace,
  };
  let created = await createWorkerHarness(options, context);
  let lane = await created.harness.lane("main", context);
  return {
    repo,
    directory,
    session,
    faux,
    state,
    entry,
    workspace,
    options,
    payloads,
    get created() {
      return created;
    },
    get lane() {
      return lane;
    },
    access(next: WorkspaceAccess) {
      access = next;
    },
    usage(usage: Usage) {
      reportedUsage = usage;
    },
    padding(text: string) {
      payloadPadding = text;
    },
    beforePayload(callback: () => void) {
      beforePayload = callback;
    },
    async reopen() {
      await created.harness.close(context);
      const reopened = await repo.open(session.metadata, context);
      created = await createWorkerHarness(
        { ...options, session: reopened },
        context,
      );
      lane = await created.harness.lane("main", context);
    },
    async prompt(text = "Inspect this task") {
      faux.setResponses([
        fauxAssistantMessage("Evidence inspected; conflict remains open."),
      ]);
      return lane.prompt(text, undefined, context);
    },
    async cleanup() {
      await created.harness.close(context);
      await repo.close(context);
      await rm(directory, { recursive: true, force: true });
    },
  };
}

describe("workspace policy (T05)", () => {
  it("round-trips the generated workspace contract through Rust", () => {
    const { state } = workingState();
    const ajv = new Ajv2020();
    ajv.addFormat("uuid", fullFormats.uuid);
    ajv.addFormat("date-time", fullFormats["date-time"]);
    const validate = ajv.compile(
      loadJson("contracts/generated/workspace-state.schema.json") as object,
    );
    expect(validate(state), JSON.stringify(validate.errors)).toBe(true);
    const rust = spawnSync(
      "cargo",
      [
        "run",
        "--quiet",
        "--example",
        "validate_contract",
        "--",
        "workspace-state",
      ],
      { input: JSON.stringify(state), encoding: "utf8" },
    );
    expect(rust.status, rust.stderr).toBe(0);
    expect(validate(JSON.parse(rust.stdout))).toBe(true);
    expect(
      validate({
        ...state,
        entries: [{ ...state.entries[0], exposure: "probably-read" }],
      }),
    ).toBe(false);
  });
  it("separates scratch lifetime, recovery retention and selected contributions", () => {
    const { state, entry } = workingState();
    state.temporary = true;
    state.entries.push({
      ...entry("scratch", "Hypothetical assumption"),
      evidential_status: "assumption",
      lifetime: { kind: "phase", phase: "explore" },
    });
    expect(formationInputs(state)).toEqual([]);
    state.selected_contributions = ["child", "scratch"];
    expect(formationInputs(state).map((e) => e.id)).toEqual(["child"]);
    state.phase = "explore";
    expect(formationInputs(state).map((e) => e.id)).toEqual([
      "child",
      "scratch",
    ]);
    expect(formationInputs(state, Date.parse(state.expires_at) + 1)).toEqual(
      [],
    );
    expect(Date.parse(state.recovery_until)).toBeGreaterThan(
      Date.parse(state.expires_at),
    );
  });

  it("revalidates active dependent conclusions without rewriting completed or differently scoped results", () => {
    const { state, entry, brief } = workingState();
    const cited = { ...emptyInputs(), sources: brief.inputs.sources };
    const interval = {
      kind: "interval" as const,
      from: "2026-01-01T00:00:00Z",
      to: "2026-02-01T00:00:00Z",
    };
    state.entries.push(
      {
        ...entry("unfinished", "Current conclusion", "hypothesis"),
        inputs: cited,
        valid_time: interval,
      },
      {
        ...entry("finished", "Completed historical report", "hypothesis"),
        inputs: cited,
        valid_time: interval,
        status: "completed",
      },
      {
        ...entry("other-time", "Later result", "hypothesis"),
        inputs: cited,
        valid_time: {
          kind: "interval",
          from: "2026-03-01T00:00:00Z",
          to: null,
        },
      },
      {
        ...entry("dependent", "Depends on unfinished result", "hypothesis"),
        supporting_entries: ["unfinished"],
      },
    );
    const change = {
      previous: cited,
      scope: brief.scope,
      validTime: interval,
      kind: "substantive" as const,
    };
    expect(reconcileWorkspace(state, [{ ...change, kind: "wording" }])).toEqual(
      state,
    );
    const result = reconcileWorkspace(state, [change]);
    expect(
      result.entries
        .filter((e) => e.status === "needs_revalidation")
        .map((e) => e.id),
    ).toEqual(["unfinished", "dependent"]);
    expect(result.entries.find((e) => e.id === "finished")).toEqual(
      state.entries.find((e) => e.id === "finished"),
    );
    expect(
      reconcileWorkspace(state, [change], Date.parse(state.expires_at) + 1),
    ).toEqual(state);
  });

  it("rejects missing conflict members and false child provenance", () => {
    const { state } = workingState();
    state.conflicts[0].members.push("absent");
    expect(() => validateWorkspace(state)).toThrow("all member entries");
    state.conflicts[0].members.pop();
    state.entries.find((e) => e.id === "child")!.generating_operation = null;
    expect(() => validateWorkspace(state)).toThrow("generating operation");
  });

  it("rejects incomplete tool exchanges", () => {
    const assistant = fauxAssistantMessage(
      fauxToolCall("read", { path: "evidence" }),
      { stopReason: "toolUse" },
    );
    expect(() => validateToolPairs([assistant])).toThrow(
      "missing tool results",
    );
    expect(() =>
      validateToolPairs([
        {
          role: "toolResult",
          toolCallId: "absent",
          toolName: "read",
          content: [],
          isError: false,
          timestamp: 0,
        },
      ]),
    ).toThrow("orphan");
  });
});

describe("workspace in the live Pi harness (T05/T03)", () => {
  it("records actual context, source coverage, conflict groups and stable layout at the provider boundary", async () => {
    const f = await fixture();
    try {
      expect(await f.prompt()).toMatchObject({
        ok: true,
        value: { status: "completed" },
      });
      const manifest = (await f.created.workspace!.latestManifest())!;
      expect(manifest.status).toBe("responded");
      expect(manifest.final_payload_bytes).toBeGreaterThan(0);
      expect(manifest.conflicts.map((g) => g.id)).toEqual(["c-conflict"]);
      expect(manifest.selected.find((e) => e.id === "child")).toMatchObject({
        exposure: "delegated_finding",
        evidential_status: "inference",
        supporting_entries: ["position-a"],
      });
      expect(manifest.sources.every((s) => s.coverage === "unexamined")).toBe(
        true,
      );
      expect(manifest.usage.cache_read_tokens).toBeNull();
      const payload = f.payloads[0] as { messages: unknown[] };
      expect(manifest.messages).toEqual(payload.messages);
      await f.prompt("Continue with the same frame");
      expect((await f.created.workspace!.latestManifest())!.strategy).toBe(
        "append",
      );
    } finally {
      await f.cleanup();
    }
  });

  it("records declared provider usage without inventing cache metrics", async () => {
    const f = await fixture();
    try {
      f.workspace.usageMetrics = ["input", "output", "cacheRead", "cacheWrite"];
      f.usage({
        input: 100,
        output: 20,
        cacheRead: 80,
        cacheWrite: 10,
        totalTokens: 210,
        cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
      });
      f.faux.setResponses([fauxAssistantMessage("Measured response")]);
      await f.lane.prompt("Measure", undefined, context);
      expect((await f.created.workspace!.latestManifest())!.usage).toEqual({
        uncached_input_tokens: 100,
        output_tokens: 20,
        cache_read_tokens: 80,
        cache_write_tokens: 10,
      });
    } finally {
      await f.cleanup();
    }
  });

  it("expires phase scratch and shields historical manifests after revocation", async () => {
    const f = await fixture();
    try {
      f.state.entries.push({
        ...f.entry("scratch", "phase-only detail"),
        lifetime: { kind: "phase", phase: "explore" },
      });
      await f.created.workspace!.checkpoint(f.state);
      await f.prompt();
      expect(JSON.stringify(f.payloads)).not.toContain("phase-only detail");
      f.access({ revision: "removed", allows: () => false });
      expect(await f.created.workspace!.latestManifest()).toMatchObject({
        status: "access_changed",
        selected: [],
        messages: [],
      });
    } finally {
      await f.cleanup();
    }
  });

  it("defers optional evidence and narrows the next step without losing required meaning", async () => {
    const f = await fixture(12_000);
    try {
      f.state.entries.push(f.entry("large-evidence", "Detail ".repeat(8_000)));
      await f.created.workspace!.checkpoint(f.state);
      expect(await f.prompt()).toMatchObject({
        ok: true,
        value: { status: "completed" },
      });
      const manifest = (await f.created.workspace!.latestManifest())!;
      expect(manifest.deferred).toContainEqual({
        entry_ids: ["large-evidence"],
        reason: "context budget",
      });
      expect(manifest.next_step).toContain("Coverage is partial");
      expect(manifest.selected.map((e) => e.id)).toEqual(
        expect.arrayContaining([
          "goal",
          "obligation",
          "position-a",
          "position-b",
        ]),
      );
      expect(manifest.final_payload_bytes).toBeLessThan(12_000);
      expect(JSON.stringify(f.payloads)).not.toContain("Detail Detail Detail");
    } finally {
      await f.cleanup();
    }
  });

  it("blocks when even required context cannot fit and records the narrowing needed", async () => {
    const f = await fixture(2_000);
    try {
      await f.prompt();
      expect(f.faux.state.callCount).toBe(0);
      expect(await f.created.workspace!.latestManifest()).toMatchObject({
        status: "blocked",
        next_step: expect.stringContaining("smaller complete task frame"),
      });
    } finally {
      await f.cleanup();
    }
  });

  it("rebuilds after revocation, including opaque history and derived findings", async () => {
    const f = await fixture();
    try {
      const source = f.state.sources[0].source;
      f.state.entries.find((e) => e.id === "position-a")!.inputs.sources = [
        source,
      ];
      f.state.entries.find((e) => e.id === "position-a")!.text =
        "restricted-detail";
      await f.created.workspace!.checkpoint(f.state);
      await f.prompt();
      await f.lane.appendMessage(
        {
          role: "user",
          content: "An old quote: restricted-detail",
          timestamp: 1,
        },
        context,
      );
      f.access({
        revision: "2",
        allows: (_scope, inputs) =>
          !inputs.sources.some((s) => s.source_id === source.source_id),
      });
      await f.prompt("Continue");
      const manifest = (await f.created.workspace!.latestManifest())!;
      expect(manifest.rebuild_reasons).toContain("access changed");
      expect(manifest.selected.map((e) => e.id)).not.toContain("child");
      expect(manifest.selected.map((e) => e.id)).not.toContain("position-b");
      expect(JSON.stringify(f.payloads.at(-1))).not.toContain(
        "restricted-detail",
      );
    } finally {
      await f.cleanup();
    }
  });

  it("checks access again immediately before the transport sends", async () => {
    const f = await fixture();
    try {
      f.beforePayload(() =>
        f.access({ revision: "revoked", allows: () => false }),
      );
      await f.prompt();
      expect(f.faux.state.callCount).toBe(0);
      expect(f.payloads).toHaveLength(0);
      expect(
        (await f.created.workspace!.latestManifest())!.next_step,
      ).toContain("Access changed");
    } finally {
      await f.cleanup();
    }
  });

  it("stops if an active obligation is inaccessible, while expired scratch can leave the frame", async () => {
    const f = await fixture();
    try {
      const obligation = f.state.entries.find((e) => e.id === "obligation")!;
      obligation.inputs.sources = [f.state.sources[0].source];
      f.state.entries.push({
        ...f.entry("expired", "Expired obligation", "obligation"),
        lifetime: { kind: "until", expires_at: "2020-01-01T00:00:00Z" },
      });
      await f.created.workspace!.checkpoint(f.state);
      await f.prompt();
      expect(
        (await f.created.workspace!.latestManifest())!.selected.map(
          (e) => e.id,
        ),
      ).not.toContain("expired");
      const calls = f.faux.state.callCount;
      f.access({
        revision: "2",
        allows: (_scope, inputs) => inputs.sources.length === 0,
      });
      await f.prompt();
      expect(f.faux.state.callCount).toBe(calls);
      expect(await f.created.workspace!.latestManifest()).toMatchObject({
        status: "blocked",
        next_step: expect.stringContaining("obligations are unavailable"),
      });
    } finally {
      await f.cleanup();
    }
  });

  it("rejects provider payload expansion and keeps transport data out of the manifest", async () => {
    const f = await fixture(12_000);
    try {
      f.padding("transport-only-secret".repeat(2_000));
      await f.prompt();
      expect(f.faux.state.callCount).toBe(0);
      const manifest = (await f.created.workspace!.latestManifest())!;
      expect(manifest.next_step).toContain("Final provider payload");
      expect(manifest.final_payload_bytes).toBeGreaterThan(12_000);
      expect(JSON.stringify(manifest)).not.toContain("transport-only-secret");
    } finally {
      await f.cleanup();
    }
  });

  it("preserves typed state and custom work results through compaction and SQLite restoration", async () => {
    const f = await fixture();
    try {
      await f.prompt();
      const result = scriptedResult("before-correction");
      await f.lane.appendCustomEntry("memory.result", result as never, context);
      await f.created.harness.setCompactionSettings(
        { enabled: true, reserveTokens: 1024, keepRecentTokens: 0 },
        context,
      );
      expect(await f.lane.compact(undefined, context)).toMatchObject({
        ok: true,
      });
      expect(
        await f.lane.findEntries({ type: "compaction" }, context),
      ).toHaveLength(1);
      await f.reopen();
      expect(await f.created.workspace!.state()).toEqual(f.state);
      await f.prompt("Continue after restoration");
      const manifest = (await f.created.workspace!.latestManifest())!;
      expect(manifest.selected.map((e) => e.id)).toEqual(
        expect.arrayContaining([
          "goal",
          "obligation",
          "position-a",
          "position-b",
          "child",
        ]),
      );
      expect(JSON.stringify(f.payloads.at(-1))).toContain(
        "Prior memory work result",
      );
      expect(JSON.stringify(f.payloads.at(-1))).toContain(
        result.examined_scope.task_id!,
      );
    } finally {
      await f.cleanup();
    }
  });

  it("restores the destination checkpoint when navigating, without importing another branch's conclusions", async () => {
    const f = await fixture();
    try {
      const destination = (await f.lane.getTipId(context))!;
      const fork = structuredClone(f.state);
      fork.entries.push(f.entry("branch-only", "Branch-only conjecture"));
      await f.created.workspace!.checkpoint(fork);
      await f.prompt();
      const calls = f.faux.state.callCount;
      expect(
        await f.lane.navigateTree(destination, { summarize: true }, context),
      ).toMatchObject({ ok: true });
      expect(f.faux.state.callCount).toBe(calls);
      expect(
        (await f.created.workspace!.state()).entries.some(
          (e) => e.id === "branch-only",
        ),
      ).toBe(false);
      await f.prompt("Continue at original checkpoint");
      expect(JSON.stringify(f.payloads.at(-1))).not.toContain(
        "Branch-only conjecture",
      );
    } finally {
      await f.cleanup();
    }
  });
});

it("keeps activation support bundles whole without reporting a false conflict", async () => {
  const f = await fixture();
  try {
    const state = await f.created.workspace!.state();
    state.conflicts = [];
    state.context_groups = [
      {
        id: "support",
        members: ["position-a", "position-b"],
        reason: "Method and exception",
      },
    ];
    await f.created.workspace!.checkpoint(state);
    f.access({
      revision: "support-hidden",
      allows: (_scope, inputs) =>
        !inputs.memories.some((r) => r.label === "hidden"),
    });
    // Hide one member while leaving its partner directly accessible.
    state.entries.find((e) => e.id === "position-b")!.inputs.memories = [
      { memory_id: randomUUID(), revision: 1, label: "hidden" },
    ];
    await f.created.workspace!.checkpoint(state);
    await f.prompt();
    const manifest = await f.created.workspace!.latestManifest();
    expect(
      manifest!.selected.some((e) =>
        ["position-a", "position-b"].includes(e.id),
      ),
    ).toBe(false);
    expect(manifest!.conflicts).toEqual([]);
  } finally {
    await f.cleanup();
  }
});
