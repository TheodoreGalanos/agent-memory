// ABOUTME: Turns a Pi session's entries into experience events for formation: what was said, asked of tools and
// ABOUTME: observed, attributed by origin and evidential status, bounded in size, with omissions made explicit.
import type { Entry } from "@earendil-works/pi-agent-core";
import type { ToolEvent } from "../../../contracts/generated/formation-window.js";
import type { WorkResult } from "../../../contracts/generated/work-result.js";

export interface ExperienceOptions {
  /** The published application result, appended as the terminal event. */
  result?: WorkResult;
  /** Longest text kept per event; longer text is cut and marked. */
  maxTextBytes?: number;
  /** Size bound for the serialised experience; later events beyond it are dropped and the drop recorded. */
  maxTotalBytes?: number;
}

const DEFAULT_TEXT_BYTES = 16_384;
const DEFAULT_TOTAL_BYTES = 768 * 1024;
const OMITTED_TAIL = "later events beyond the experience size limit";

export function experienceEvents(entries: Entry[], options: ExperienceOptions = {}): ToolEvent[] {
  const maxText = options.maxTextBytes ?? DEFAULT_TEXT_BYTES;
  const maxTotal = options.maxTotalBytes ?? DEFAULT_TOTAL_BYTES;
  const events: ToolEvent[] = [];
  const clip = (text: string) => {
    const buffer = Buffer.from(text, "utf8");
    if (buffer.byteLength <= maxText) return { text, truncated: false };
    return { text: buffer.subarray(0, maxText).toString("utf8").replace(/\uFFFD+$/u, ""), truncated: true };
  };
  const push = (entry: Entry, kind: string, origin: ToolEvent["origin"], status: ToolEvent["evidential_status"], content: Record<string, unknown>, suffix = "") => {
    events.push({
      event_id: `${entry.seq}-${kind}${suffix}`,
      observed_at: new Date(entry.timestamp).toISOString(),
      kind,
      origin,
      evidential_status: status,
      content,
    });
  };
  for (const entry of [...entries].sort((a, b) => a.seq - b.seq)) {
    if (entry.type === "compaction" || entry.type === "branch_summary") {
      push(entry, "summary", "agent_generated", "inference", { ...clip(entry.summary) });
      continue;
    }
    if (entry.type !== "message") continue; // Rendered context and application markers are not experience.
    const message = entry.message;
    if (message.role === "user") {
      const parts = typeof message.content === "string" ? [{ type: "text" as const, text: message.content }] : message.content;
      const text = parts.filter((p) => p.type === "text").map((p) => p.text).join("\n");
      const omitted = parts.filter((p) => p.type !== "text").map((p) => p.type);
      push(entry, "message", "observed", "attributed_statement", { role: "user", ...clip(text), ...(omitted.length ? { omitted } : {}) });
    } else if (message.role === "assistant") {
      const text = message.content.filter((p) => p.type === "text").map((p) => p.text).join("\n");
      if (text.trim()) push(entry, "message", "agent_generated", "inference", { role: "assistant", ...clip(text) });
      for (const [index, call] of message.content.filter((p) => p.type === "toolCall").entries())
        push(entry, "tool_call", "agent_generated", "inference", { tool: call.name, call_id: call.id, arguments: bounded(call.arguments, maxText) }, index ? `-${index}` : "");
    } else if (message.role === "toolResult") {
      const text = message.content.filter((p) => p.type === "text").map((p) => p.text).join("\n");
      const omitted = message.content.filter((p) => p.type !== "text").map((p) => p.type);
      push(entry, "tool_result", "observed", "observation", { tool: message.toolName, call_id: message.toolCallId, is_error: message.isError, ...clip(text), ...(omitted.length ? { omitted } : {}) });
    }
  }
  if (options.result) {
    const last = entries.at(-1);
    events.push({
      event_id: `${(last?.seq ?? 0) + 1}-result`,
      observed_at: new Date(last?.timestamp ?? Date.now()).toISOString(),
      kind: "result",
      origin: "agent_generated",
      evidential_status: "inference",
      content: { result: options.result },
    });
  }
  return bound(events, maxTotal);
}

/** Keeps a prefix of the events within the size bound and marks the cut on the last kept event. */
function bound(events: ToolEvent[], maxTotal: number): ToolEvent[] {
  let size = 2;
  for (const [index, event] of events.entries()) {
    size += Buffer.byteLength(JSON.stringify(event)) + 1;
    if (size > maxTotal) {
      const kept = events.slice(0, index);
      const last = kept.at(-1);
      if (!last) return [];
      const omitted = [...((last.content as { omitted?: string[] }).omitted ?? []), OMITTED_TAIL];
      kept[kept.length - 1] = { ...last, content: { ...(last.content as object), omitted } };
      return kept;
    }
  }
  return events;
}

/** Tool arguments are kept as given unless their JSON is too long, in which case a marked text stand-in replaces them. */
function bounded(value: Record<string, unknown>, maxText: number): unknown {
  const text = JSON.stringify(value);
  return Buffer.byteLength(text) <= maxText ? value : { truncated: true, text: text.slice(0, maxText) };
}
