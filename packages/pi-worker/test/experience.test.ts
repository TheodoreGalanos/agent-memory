// ABOUTME: Tests for turning a Pi session into experience events: ordinary messages, tool calls and results
// ABOUTME: become bounded, attributed tool events; thinking, images and rendered context are left out.
import { expect, it } from "vitest";
import type { Entry } from "@earendil-works/pi-agent-core";
import { experienceEvents } from "../src/experience.js";

let seq = 0;
const entry = (e: Record<string, unknown>): Entry =>
  ({ id: `e${++seq}`, parentId: null, seq, timestamp: 1_700_000_000_000 + seq * 1000, ...e }) as unknown as Entry;

const transcript: Entry[] = [
  entry({ type: "message", message: { role: "user", content: "Find the clearance under the beam.", timestamp: 1 } }),
  entry({ type: "custom", customType: "memory.workspace", data: { entries: [] } }),
  entry({ type: "message", message: {
    role: "assistant", api: "x", provider: "faux", model: "m", usage: {}, stopReason: "toolUse", timestamp: 2,
    content: [
      { type: "thinking", thinking: "private chain of thought" },
      { type: "text", text: "I will read the drawing." },
      { type: "toolCall", id: "call-1", name: "read", arguments: { path: "drawing.txt" } },
    ],
  } }),
  entry({ type: "message", message: { role: "toolResult", toolCallId: "call-1", toolName: "read", isError: false, timestamp: 3,
    content: [{ type: "text", text: "Clearance: 3 m" }, { type: "image", data: "...", mimeType: "image/png" }] } }),
  entry({ type: "compaction", summary: "Earlier turns summarised.", retainedTail: [], tokensBefore: 10, fromHook: false }),
  entry({ type: "message", message: { role: "assistant", api: "x", provider: "faux", model: "m", usage: {}, stopReason: "stop", timestamp: 4,
    content: [{ type: "text", text: "The clearance is 3 m." }] } }),
];

it("maps a session to attributed events in order, dropping thinking, images and rendered context", () => {
  const events = experienceEvents(transcript);
  expect(events.map((e) => e.kind)).toEqual(["message", "message", "tool_call", "tool_result", "summary", "message"]);
  expect(events[0]).toMatchObject({ origin: "observed", evidential_status: "attributed_statement", content: { role: "user", text: "Find the clearance under the beam." } });
  expect(events[1]).toMatchObject({ origin: "agent_generated", evidential_status: "inference", content: { role: "assistant", text: "I will read the drawing." } });
  expect(events[2]).toMatchObject({ origin: "agent_generated", evidential_status: "inference", content: { tool: "read", call_id: "call-1", arguments: { path: "drawing.txt" } } });
  expect(events[3]).toMatchObject({ origin: "observed", evidential_status: "observation", content: { tool: "read", call_id: "call-1", is_error: false, text: "Clearance: 3 m", omitted: ["image"] } });
  expect(events[4]).toMatchObject({ origin: "agent_generated", evidential_status: "inference", content: { text: "Earlier turns summarised." } });
  expect(JSON.stringify(events)).not.toContain("private chain of thought");
  expect(new Set(events.map((e) => e.event_id)).size).toBe(events.length);
  expect(events.every((e) => !Number.isNaN(Date.parse(e.observed_at)))).toBe(true);
});

it("appends the application result as the terminal event when supplied", () => {
  const result = { schema_version: "1", status: "complete", findings: [] } as never;
  const events = experienceEvents(transcript, { result });
  expect(events.at(-1)).toMatchObject({ kind: "result", origin: "agent_generated", evidential_status: "inference", content: { result: { status: "complete" } } });
});

it("bounds each event's text and the whole experience deterministically", () => {
  const long = "x".repeat(20_000);
  const many: Entry[] = Array.from({ length: 300 }, () =>
    entry({ type: "message", message: { role: "toolResult", toolCallId: "c", toolName: "bash", isError: false, timestamp: 1, content: [{ type: "text", text: long }] } }),
  );
  const events = experienceEvents(many, { maxTextBytes: 4_096, maxTotalBytes: 200_000 });
  expect(events.length).toBeLessThan(300);
  expect(events.every((e) => Buffer.byteLength(String((e.content as { text: string }).text)) <= 4_096)).toBe(true);
  expect((events[0].content as { truncated: boolean }).truncated).toBe(true);
  expect(Buffer.byteLength(JSON.stringify(events))).toBeLessThanOrEqual(200_000);
  // Truncation is recorded on the last kept event so the omission is visible, not silent.
  expect((events.at(-1)!.content as { omitted: string[] }).omitted).toContain("later events beyond the experience size limit");
});
