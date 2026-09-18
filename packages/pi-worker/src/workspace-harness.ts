import { randomUUID } from "node:crypto";
import {
  value,
  convertToLlm,
  type AgentHarness,
  type AgentLane,
  type AgentMessage,
  type Context,
  type EntryProjector,
  type ExecutionToolContext,
  type JsonValue,
  type Session,
} from "@earendil-works/pi-agent-core";
import type {
  ConfigRef,
  RenderManifest,
} from "../../../contracts/generated/render-manifest.js";
import {
  alive,
  emptyInputs,
  validateWorkspace,
  type WorkspaceAccess,
  type WorkspaceState,
} from "./workspace.js";

export interface WorkspaceOptions {
  onManifest?: (manifest: RenderManifest) => Promise<void>;
  initial: WorkspaceState;
  profile: ConfigRef;
  maxPayloadBytes: number;
  /** Capacity reserved for the next tool result, in addition to model output. */
  reservedResultTokens?: number;
  readAccess: () => Promise<WorkspaceAccess>;
  refresh?: (state: WorkspaceState) => Promise<WorkspaceState>;
  /** Only enable metrics the selected provider actually reports. */
  usageMetrics?: ("input" | "output" | "cacheRead" | "cacheWrite")[];
}

const checkpointType = "memory.workspace";
const marker =
  "Workspace checkpoint; current working state is supplied by the workspace renderer.";
export const workspaceProjector: EntryProjector = (entry) => [
  { role: "user", content: marker, timestamp: entry.timestamp },
];
export const resultProjector: EntryProjector = (entry) => [
  {
    role: "user",
    content: `Prior memory work result: ${JSON.stringify(entry.data)}`,
    timestamp: entry.timestamp,
  },
];
const json = (object: unknown) =>
  JSON.parse(JSON.stringify(object)) as JsonValue;
const bytes = (object: unknown) => Buffer.byteLength(JSON.stringify(object));
const address = (id: string) => value<RenderManifest>("memory.render", id);
const latestAddress = (lane: string) =>
  value<string>("memory.render.latest", lane);

/** Keep whole user/assistant/tool rounds. A partial tool exchange is never emitted. */
function rounds(messages: AgentMessage[]): AgentMessage[][] {
  const result: AgentMessage[][] = [];
  for (const message of messages) {
    if (message.role === "user" || !result.length) result.push([]);
    result.at(-1)!.push(message);
  }
  return result;
}

export function validateToolPairs(messages: AgentMessage[]): void {
  const pending = new Set<string>();
  for (const message of messages) {
    if (message.role === "toolResult") {
      if (!pending.delete(message.toolCallId))
        throw new Error("Provider context has an orphan tool result");
    } else {
      if (pending.size)
        throw new Error("Provider context has missing tool results");
      if (message.role === "assistant")
        for (const part of message.content)
          if (part.type === "toolCall") {
            if (pending.has(part.id)) throw new Error("Duplicate tool call ID");
            pending.add(part.id);
          }
    }
  }
  if (pending.size)
    throw new Error("Provider context has missing tool results");
}

export async function attachWorkspace(
  harness: AgentHarness<ExecutionToolContext>,
  session: Session,
  options: WorkspaceOptions,
  context: Context,
) {
  validateWorkspace(options.initial);
  if (
    !Number.isSafeInteger(options.maxPayloadBytes) ||
    options.maxPayloadBytes <= 0 ||
    !Number.isSafeInteger(options.reservedResultTokens ?? 1024) ||
    (options.reservedResultTokens ?? 1024) < 0
  ) {
    throw new Error("Invalid workspace context budget");
  }
  async function load(
    lane: AgentLane,
  ): Promise<{ state: WorkspaceState; checkpointId: string } | undefined> {
    const entries = await lane.findEntries({ order: "newestFirst" }, context);
    for (const entry of entries) {
      if (entry.type === "custom" && entry.customType === checkpointType) {
        const saved = await session.getValue(
          value<WorkspaceState>("memory.workspace.refresh", entry.id),
          context,
        );
        return {
          state: saved?.value ?? (entry.data as unknown as WorkspaceState),
          checkpointId: entry.id,
        };
      }
      if (
        entry.type === "compaction" &&
        entry.details &&
        typeof entry.details === "object" &&
        !Array.isArray(entry.details) &&
        entry.details.workspace
      ) {
        const saved = await session.getValue(
          value<WorkspaceState>("memory.workspace.refresh", entry.id),
          context,
        );
        return {
          state:
            saved?.value ??
            (entry.details.workspace as unknown as WorkspaceState),
          checkpointId: entry.id,
        };
      }
    }
    return undefined;
  }
  async function state(laneName = "main", refresh = false) {
    const lane = await harness.lane(laneName, context);
    const current = await load(lane);
    if (!current)
      throw new Error("Lane has no workspace; supply an explicit child frame");
    if (refresh && options.refresh) {
      current.state = await options.refresh(current.state);
      validateWorkspace(current.state);
      // Updating working state must not append transcript entries while Pi builds a request.
      // That changes the lane head and makes Pi repeat the provider step.
      await session.setValue(
        value<WorkspaceState>("memory.workspace.refresh", current.checkpointId),
        current.state,
        context,
      );
    }
    validateWorkspace(current.state);
    if (Date.parse(current.state.recovery_until) <= Date.now())
      throw new Error("Workspace recovery retention has expired");
    return current.state;
  }
  async function checkpoint(next: WorkspaceState, laneName = "main") {
    validateWorkspace(next);
    const lane = await harness.lane(laneName, context);
    await lane.appendCustomEntry(checkpointType, json(next), context);
  }
  const main = await harness.lane("main", context);
  if (!(await load(main))) {
    // A fresh workspace is only installed at an empty root. Existing sessions need
    // an explicit checkpoint so that history is not mistaken for a complete frame.
    if ((await main.findEntries(undefined, context)).length)
      throw new Error(
        "Existing session needs an explicit workspace checkpoint",
      );
    await checkpoint(options.initial);
  }
  async function latest(
    laneName = "main",
  ): Promise<RenderManifest | undefined> {
    const id = (await session.getValue(latestAddress(laneName), context))
      ?.value;
    return id
      ? (await session.getValue(address(id), context))?.value
      : undefined;
  }
  async function save(manifest: RenderManifest) {
    await options.onManifest?.(manifest);
    await session.setValue(address(manifest.decision_id), manifest, context);
    await session.setValue(
      latestAddress(manifest.lane),
      manifest.decision_id,
      context,
    );
  }

  harness.hooks.on(
    "transform_context",
    async ({ lane: laneName, runId, messages, systemPrompt }) => {
      const lane = await harness.lane(laneName, context);
      const current = await state(laneName, true);
      const now = Date.now();
      if (Date.parse(current.expires_at) <= now)
        throw new Error("Workspace purpose has expired");
      const access = await options.readAccess();
      const previous = await latest(laneName);
      const model = await lane.getModel(context);
      if (!model) throw new Error("Workspace model is unavailable");
      const activeTools = await lane.getActiveTools(context);
      const toolSchemas = (await harness.getTools(context))
        .filter((t) => activeTools.includes(t.name))
        .map((t) => ({
          name: t.name,
          description: t.description,
          parameters: t.parameters,
        }));
      const reserve = options.reservedResultTokens ?? 1024;
      // One UTF-8 byte per token is deliberately conservative for text. The final
      // provider payload has a separate check; this is not an exact tokenizer.
      const limit = Math.min(
        options.maxPayloadBytes,
        model.contextWindow - model.maxTokens - reserve,
      );
      const overhead = bytes({ systemPrompt, tools: toolSchemas }) + 1024;
      const manifest: RenderManifest = {
        schema_version: "1",
        decision_id: randomUUID(),
        workspace_id: current.workspace_id,
        session_id: session.metadata.id,
        lane: laneName,
        operation_id: runId,
        profile: options.profile,
        provider: model.provider,
        model: model.id,
        access_revision: access.revision,
        selected: [],
        sources: [],
        conflicts: [],
        deferred: [],
        next_step: null,
        messages: [],
        tool_schemas: JSON.parse(JSON.stringify(toolSchemas)),
        strategy: "append",
        rebuild_reasons: [],
        estimator:
          "UTF-8 bytes as token upper estimate; 1024-byte provider margin",
        estimated_input_tokens: 0,
        reserved_output_tokens: model.maxTokens,
        reserved_result_tokens: reserve,
        final_payload_bytes: null,
        status: "prepared",
        usage: {
          uncached_input_tokens: null,
          cache_read_tokens: null,
          cache_write_tokens: null,
          output_tokens: null,
        },
      };
      const all = new Map(current.entries.map((e) => [e.id, e]));
      const allowed = new Map<string, boolean>();
      const hiddenGroupMembers = new Set<string>();
      function visible(id: string): boolean {
        if (allowed.has(id)) return allowed.get(id)!;
        const e = all.get(id)!;
        const yes =
          !hiddenGroupMembers.has(id) &&
          alive(current, e, now) &&
          access.allows(e.scope, e.inputs) &&
          e.supporting_entries.every(visible);
        allowed.set(id, yes);
        return yes;
      }
      const groups: string[][] = [];
      const grouped = new Set<string>();
      // Keep conflicts and activation support bundles whole under access/budget limits.
      for (const conflict of [
        ...current.conflicts.filter((g) => g.unresolved),
        ...(current.context_groups ?? []),
      ]) {
        let members = new Set(conflict.members);
        for (let i = groups.length - 1; i >= 0; i--)
          if (groups[i].some((id) => members.has(id))) {
            for (const id of groups.splice(i, 1)[0]) members.add(id);
          }
        groups.push([...members]);
      }
      for (const group of groups) {
        if (!group.every(visible))
          group.forEach((id) => hiddenGroupMembers.add(id));
        group.forEach((id) => grouped.add(id));
      }
      allowed.clear();
      const available = current.entries.filter((e) => visible(e.id));
      for (const e of available) if (!grouped.has(e.id)) groups.push([e.id]);
      const required = (ids: string[]) =>
        ids.some((id) => {
          const e = all.get(id)!;
          return (
            e.status !== "completed" &&
            (["goal", "constraint", "obligation"].includes(e.kind) ||
              e.decision_relevant ||
              e.status === "needs_revalidation")
          );
        }) ||
        current.conflicts.some(
          (g) => g.unresolved && g.members.some((id) => ids.includes(id)),
        );
      const content = () => ({
        workspace_id: current.workspace_id,
        scope: current.scope,
        phase: current.phase,
        temporary: current.temporary,
        selected_contributions: current.selected_contributions.filter((id) =>
          manifest.selected.some((e) => e.id === id),
        ),
        entries: manifest.selected,
        sources: manifest.sources,
        conflicts: manifest.conflicts,
        context_groups: (current.context_groups ?? []).filter((g) =>
          g.members.every((id) => manifest.selected.some((e) => e.id === id)),
        ),
        deferred: manifest.deferred,
        unavailable_context:
          hiddenGroupMembers.size > 0 ||
          current.entries.some(
            (e) =>
              e.decision_relevant && alive(current, e, now) && !visible(e.id),
          ),
        next_step: manifest.next_step,
      });
      const frame = (): AgentMessage => ({
        role: "user",
        content: JSON.stringify(content()),
        timestamp: 0,
      });
      const fits = (tail: AgentMessage[] = []) =>
        overhead + bytes([frame(), ...tail]) <= limit;
      async function block(reason: string): Promise<never> {
        manifest.status = "blocked";
        manifest.next_step = reason;
        manifest.messages = [];
        manifest.estimated_input_tokens = overhead + bytes([frame()]);
        await save(manifest);
        throw new Error(reason);
      }
      if (
        current.entries.some(
          (e) =>
            ["goal", "constraint", "obligation"].includes(e.kind) &&
            e.status === "active" &&
            alive(current, e, now) &&
            !visible(e.id),
        ) ||
        !available.some((e) => e.kind === "goal" && e.status === "active")
      ) {
        return block(
          "Required task instructions or obligations are unavailable. Obtain a permitted task frame before continuing.",
        );
      }
      groups.sort(
        (a, b) =>
          Number(required(b)) - Number(required(a)) ||
          Math.max(...b.map((id) => all.get(id)!.priority)) -
            Math.max(...a.map((id) => all.get(id)!.priority)),
      );
      for (const group of groups) {
        if (!group.every(visible)) {
          manifest.rebuild_reasons.push("required context unavailable");
          manifest.next_step =
            "Some context is unavailable. Resolve access or request permitted evidence before relying on affected claims.";
          continue;
        }
        const entries = group.map((id) => all.get(id)!);
        manifest.selected.push(...entries);
        const conflicts = current.conflicts.filter((g) =>
          g.members.every((id) => manifest.selected.some((e) => e.id === id)),
        );
        manifest.conflicts = conflicts;
        if (!fits()) {
          if (required(group))
            return block(
              "Required context exceeds the profile. Supply a smaller complete task frame or concise conflict statements with detail references.",
            );
          manifest.selected.splice(-entries.length);
          manifest.conflicts = current.conflicts.filter((g) =>
            g.members.every((id) => manifest.selected.some((e) => e.id === id)),
          );
          manifest.deferred.push({
            entry_ids: group,
            reason: "context budget",
          });
        }
      }
      // The inventory describes availability, not a claim that the agent read it.
      for (const source of current.sources) {
        if (
          !access.allows(current.scope, {
            ...emptyInputs(),
            sources: [source.source],
          })
        )
          continue;
        manifest.sources.push(source);
        if (!fits()) {
          manifest.sources.pop();
          manifest.deferred.push({
            entry_ids: [],
            reason: "source inventory detail deferred by context budget",
          });
          break;
        }
      }
      if (manifest.deferred.length)
        manifest.next_step = [
          manifest.next_step,
          "Inspect the next deferred evidence group before making a conclusion that depends on it. Coverage is partial.",
        ]
          .filter(Boolean)
          .join(" ");
      const reasons = manifest.rebuild_reasons;
      if (!previous) reasons.push("initial frame");
      else {
        if (previous.access_revision !== access.revision)
          reasons.push("access changed");
        if (
          JSON.stringify(previous.sources) !== JSON.stringify(manifest.sources)
        )
          reasons.push("source inventory changed");
        const old = previous.selected;
        if (
          old.some(
            (e, i) =>
              JSON.stringify(manifest.selected[i]) !== JSON.stringify(e),
          )
        )
          reasons.push("working context changed");
      }
      // Projectors insert a boundary, not an opaque cached copy of the working state.
      let boundary = -1;
      messages.forEach((m, i) => {
        if (
          (m.role === "user" && m.content === marker) ||
          m.role === "compactionSummary" ||
          m.role === "branchSummary"
        )
          boundary = i;
      });
      let tail = messages.slice(boundary + 1);
      if (
        reasons.some(
          (r) =>
            r === "access changed" ||
            r === "working context changed" ||
            r === "required context unavailable",
        )
      ) {
        tail = [];
        reasons.push("transcript rebuilt from current workspace");
      }
      const recent = rounds(tail);
      while (recent.length && !fits(recent.flat())) {
        recent.shift();
        reasons.push("transcript budget");
      }
      if (reasons.includes("transcript budget"))
        manifest.next_step = [
          manifest.next_step,
          "Earlier conversation is omitted. Continue from this workspace and inspect referenced evidence before using omitted details.",
        ]
          .filter(Boolean)
          .join(" ");
      // Deferral notices themselves consume space. Never silently shorten required meaning.
      if (!fits(recent.flat()))
        return block(
          "Context and deferrals exceed the profile. Narrow the task frame before the next provider request.",
        );
      const projected = [frame(), ...recent.flat()];
      if (
        projected.some(
          (m) =>
            "content" in m &&
            Array.isArray(m.content) &&
            m.content.some((p) => p.type === "image"),
        )
      ) {
        return block(
          "This text profile cannot estimate image input. Use a qualified multimodal profile.",
        );
      }
      validateToolPairs(projected);
      // Let Pi do its supported conversion after this hook, but record the same public transcript now.
      manifest.messages = JSON.parse(
        JSON.stringify([
          ...(systemPrompt
            ? [{ role: "system", content: systemPrompt, timestamp: 0 }]
            : []),
          ...convertToLlm(projected),
        ]),
      );
      manifest.estimated_input_tokens = overhead + bytes(manifest.messages);
      manifest.rebuild_reasons = [...new Set(reasons)];
      manifest.strategy = reasons.length ? "rebuild" : "append";
      await save(manifest);
      return { messages: projected };
    },
  );

  harness.hooks.on(
    "before_payload",
    async ({ lane, runId, payload, model }) => {
      const manifest = await latest(lane);
      if (!manifest) throw new Error("Provider request has no render manifest");
      const current = await state(lane);
      const access = await options.readAccess();
      manifest.final_payload_bytes = bytes(payload);
      const limit = Math.min(
        options.maxPayloadBytes,
        model.contextWindow - model.maxTokens - manifest.reserved_result_tokens,
      );
      if (
        manifest.operation_id !== runId ||
        Date.parse(current.expires_at) <= Date.now()
      ) {
        manifest.status = "blocked";
        manifest.next_step =
          "Workspace purpose or operation changed. Rebuild an active task frame before requesting the provider.";
      } else if (
        access.revision !== manifest.access_revision ||
        manifest.selected.some((e) => !access.allows(e.scope, e.inputs)) ||
        manifest.sources.some(
          (s) =>
            !access.allows(current.scope, {
              ...emptyInputs(),
              sources: [s.source],
            }),
        )
      ) {
        manifest.status = "blocked";
        manifest.next_step =
          "Access changed during rendering. Rebuild context before requesting the provider.";
      } else if (manifest.final_payload_bytes > limit) {
        manifest.status = "blocked";
        manifest.next_step =
          "Final provider payload exceeds the profile. Narrow the context or tool set before retrying.";
      } else manifest.status = "payload_checked";
      await save(manifest);
      if (manifest.status === "blocked") throw new Error(manifest.next_step!);
      return undefined;
    },
  );
  harness.hooks.on("after_response", async ({ lane, message }) => {
    const manifest = await latest(lane);
    if (!manifest) return;
    if (message.stopReason === "error" || message.stopReason === "aborted") {
      if (manifest.status !== "blocked") manifest.status = "provider_failed";
    } else {
      manifest.status =
        manifest.final_payload_bytes == null
          ? "response_without_payload_hook"
          : "responded";
      const metrics = options.usageMetrics ?? [];
      const measured = (
        key: "input" | "output" | "cacheRead" | "cacheWrite",
      ) => (metrics.includes(key) ? message.usage[key] : null);
      manifest.usage = {
        uncached_input_tokens: measured("input"),
        output_tokens: measured("output"),
        cache_read_tokens: measured("cacheRead"),
        cache_write_tokens: measured("cacheWrite"),
      };
    }
    await save(manifest);
    return undefined;
  });
  harness.hooks.on("before_compaction", async ({ lane, preparation }) => {
    const current = await state(lane);
    // Carry structural working state through Pi's own compaction settlement. Raw
    // transcript tails remain subject to access and budget checks on the next run.
    const branch = await harness.lane(lane, context);
    const results = (
      await branch.findEntries({ type: "custom" }, context)
    ).filter((e) => e.type === "custom" && e.customType === "memory.result");
    const retainedTail = [...preparation.retainedTail];
    for (const result of results)
      if (result.type === "custom") {
        const projected = (await resultProjector(result, context)) ?? [];
        for (const message of projected)
          if (
            !retainedTail.some(
              (m) => JSON.stringify(m) === JSON.stringify(message),
            )
          )
            retainedTail.push(message);
      }
    return {
      compaction: {
        summary: marker,
        tokensBefore: preparation.tokensBefore,
        retainedTail,
        details: json({ workspace: current }),
      },
    };
  });
  harness.hooks.on("before_navigation", () => ({
    summary: {
      summary:
        "Branch context was not imported. Continue from the destination workspace checkpoint.",
      readFiles: [],
      modifiedFiles: [],
    },
  }));
  async function inspectManifest(laneName = "main") {
    const manifest = await latest(laneName);
    if (!manifest) return undefined;
    const access = await options.readAccess();
    if (
      access.revision === manifest.access_revision &&
      manifest.selected.every((e) => access.allows(e.scope, e.inputs))
    )
      return manifest;
    // An old transcript can contain paraphrases of revoked evidence. The trusted
    // session retains it for administration; a task-context view does not expose it.
    return {
      ...manifest,
      selected: [],
      sources: [],
      conflicts: [],
      deferred: [],
      messages: [],
      access_revision: access.revision,
      status: "access_changed",
      next_step:
        "Access changed. Render the current workspace before inspecting its context.",
    };
  }
  return { state, checkpoint, latestManifest: inspectManifest };
}
