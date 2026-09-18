// ABOUTME: Compact operator trace for judgement calls made by pool workers: which provider was
// ABOUTME: asked what, and what it answered, with latency and tokens but without raw payloads.
import { performance } from "node:perf_hooks";
import type { JudgementPacket } from "../../../contracts/generated/judgement-packet.js";
import type { JudgementUsage } from "../../../contracts/generated/formation-result.js";
import type { JudgementProvider } from "../../judgement/src/providers.js";
import { questionMap } from "../../judgement/src/validation.js";

const clip = (text: string, length = 60) => (text.length > length ? `${text.slice(0, length - 1)}…` : text);

function answersOf(raw: unknown): string {
  if (!raw || typeof raw !== "object" || !("answers" in raw) || !raw.answers || typeof raw.answers !== "object")
    return "no structured answers";
  return Object.entries(raw.answers as Record<string, unknown>)
    .map(([key, answer]) => {
      if (answer && typeof answer === "object") {
        const a = answer as Record<string, unknown>;
        if ("choice" in a) return `${key}=${String(a.choice)}`;
        if ("noul" in a) return `${key}=${String(a.noul)}`;
        if ("score" in a) return `${key}=${String(a.score)}`;
        if ("distribution" in a) return `${key}=${JSON.stringify(a.distribution)}`;
      }
      return `${key}=${clip(JSON.stringify(answer))}`;
    })
    .join(", ");
}

export function formatReply(
  role: string,
  provider: { id: string; model: string },
  reply: { status: number; raw: unknown; usage: JudgementUsage },
  latencyMs: number,
): string {
  const tokens = `tokens in ${reply.usage.input_tokens ?? "?"} / out ${reply.usage.output_tokens ?? "?"}`;
  return `← ${role} ${provider.id}: ${answersOf(reply.raw)} (HTTP ${reply.status}, ${latencyMs} ms, ${tokens})`;
}

/** Logs each evaluation around the wrapped provider without changing its answers. */
export function observeProvider(provider: JudgementProvider, role: string, log: (line: string) => void): JudgementProvider {
  return {
    id: provider.id,
    model: provider.model,
    release: provider.release,
    maximum: provider.maximum,
    distributions: provider.distributions,
    async evaluate(packet: JudgementPacket, signal: AbortSignal) {
      log(`→ ${role} ${provider.id} (${provider.model}) asked ${Object.keys(questionMap(packet)).join(", ")} about "${packet.subject}"`);
      const start = performance.now();
      try {
        const reply = await provider.evaluate(packet, signal);
        log(formatReply(role, provider, reply, Math.round(performance.now() - start)));
        return reply;
      } catch (error) {
        log(`← ${role} ${provider.id} failed: ${error instanceof Error ? error.message : String(error)}`);
        throw error;
      }
    },
  };
}
