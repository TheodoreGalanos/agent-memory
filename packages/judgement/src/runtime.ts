import { randomUUID } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import { setTimeout as delay } from "node:timers/promises";
import {
  value,
  type Context,
  type Session,
} from "@earendil-works/pi-agent-core";
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type {
  JudgementPacket,
  Scope,
} from "../../../contracts/generated/judgement-packet.js";
import type { SemanticAssessment } from "../../../contracts/generated/semantic-assessment.js";
import type { JudgementDecision } from "../../../contracts/generated/judgement-decision.js";
import type { HostCommands } from "../../pi-worker/src/host-client.js";
import { permits } from "../../pi-worker/src/workspace.js";
import { fenceFor } from "./packet.js";
import {
  decide,
  qualified,
  inconsistencies,
  type ConsistencyRule,
  type Qualification,
} from "./policy.js";
import { validateResponse } from "./validation.js";
import type { JudgementProvider, ProviderReply } from "./providers.js";

export interface JudgementOptions {
  reference: JudgementProvider;
  jev?: JudgementProvider;
  mode?: "reference" | "shadow" | "qualified";
  qualifications?: Qualification[];
  consistencyRules?: ConsistencyRule[];
  /** Trusted operator configuration; not obtained from model output. */
  disclosure: { policy: string; scope: Scope; providers: string[] };
  maxAttempts?: number;
  reuseAssessmentId?: string;
}
interface Attempt {
  provider: string;
  id: string;
  rawId: string;
  assessmentId: string;
  reservationId?: string;
  started?: boolean;
  reply?: ProviderReply;
  prepared?: SemanticAssessment;
  assessment?: SemanticAssessment;
}
interface RunState {
  packet: JudgementPacket;
  configuration: unknown;
  attempts: Attempt[];
  decisionId: string;
  decision?: JudgementDecision;
}

/** Run under the assigned worker's lease keeper. Each network attempt has its
 * own reservation; saved replies can be finalised without another provider call. */
export async function runJudgement(
  host: HostCommands,
  assignment: Assignment,
  session: Session,
  packet: JudgementPacket,
  options: JudgementOptions,
  context: Context,
): Promise<JudgementDecision> {
  const fence = fenceFor(assignment),
    mode = options.mode ?? (options.jev ? "shadow" : "reference");
  const limit = options.maxAttempts ?? 3;
  if (!Number.isInteger(limit) || limit < 1 || limit > 3)
    throw new Error(
      "Judgement attempts must be between one and three per provider",
    );
  if (mode !== "reference" && !options.jev)
    throw new Error("Jev mode needs a Jev provider");
  if (options.reference.id === options.jev?.id)
    throw new Error("Reference and Jev need distinct provider identities");
  const configuration = {
    mode,
    limit,
    disclosure: options.disclosure,
    qualifications: options.qualifications ?? [],
    consistencyRules: options.consistencyRules ?? [],
    providers: [options.reference, ...(options.jev ? [options.jev] : [])].map(
      (p) => ({
        id: p.id,
        model: p.model,
        release: p.release ?? null,
        maximum: p.maximum,
      }),
    ),
  };
  const rules = options.consistencyRules ?? [];
  for (const rule of rules) {
    if (
      !rule.id.trim() ||
      rule.allOf.length < 2 ||
      rule.allOf.some((condition) => {
        const question = packet.questions
          .flatMap((d) =>
            d.definition.questions.map((q) => ({
              key: `${d.definition.id}.${q.key}`,
              form: q.form,
            })),
          )
          .find((q) => q.key === condition.question);
        return (
          !question ||
          question.form.type !== "choice" ||
          !Object.hasOwn(question.form.criteria, condition.choice)
        );
      })
    )
      throw new Error(
        "Consistency rules must refer to declared alternatives in this packet",
      );
  }
  const address = value<RunState>("memory.judgement", packet.id);
  let state = (await session.getValue(address, context))?.value;
  if (
    state &&
    (!isDeepStrictEqual(state.packet, packet) ||
      !isDeepStrictEqual(state.configuration, configuration))
  )
    throw new Error(
      "An admitted judgement run changed; create a new packet to reevaluate policy",
    );
  if (!state) {
    state = { packet, configuration, attempts: [], decisionId: randomUUID() };
    await session.setValue(address, state, context);
  }
  const saved = state;
  const save = () => session.setValue(address, saved, context);
  async function check(provider: JudgementProvider) {
    context.abortSignal?.throwIfAborted();
    if (Date.now() >= Date.parse(packet.deadline))
      throw new Error("Judgement deadline expired");
    if (
      options.disclosure.policy !== packet.disclosure_policy ||
      !permits(options.disclosure.scope, packet.scope) ||
      !options.disclosure.providers.includes(provider.id) ||
      !packet.allowed_providers.includes(provider.id)
    )
      throw new Error("Provider disclosure is not authorised");
    const response = await host.request(
      {
        action: "check_judgement",
        fence,
        packet_id: packet.id,
        provider: provider.id,
      },
      context.abortSignal,
    );
    if (
      response.kind !== "judgement_packet" ||
      !isDeepStrictEqual(response.packet, JSON.parse(JSON.stringify(packet)))
    )
      throw new Error("The admitted packet is unavailable or changed");
  }
  async function assess(
    provider: JudgementProvider,
  ): Promise<SemanticAssessment> {
    await check(provider);
    if (provider.maximum.provider_calls !== 1 || provider.maximum.tokens <= 0)
      throw new Error(
        "Each provider attempt needs one call and a positive token allowance",
      );
    if (options.reuseAssessmentId && provider.release) {
      const reused = await host.request(
        {
          action: "reuse_assessment",
          fence,
          assessment_id: options.reuseAssessmentId,
          packet_id: packet.id,
          provider: provider.id,
          model_release: provider.release,
        },
        context.abortSignal,
      );
      if (reused.kind === "assessment" && reused.assessment)
        return reused.assessment;
    }
    for (let n = 0; n < limit; n++) {
      let attempt = saved.attempts.filter((a) => a.provider === provider.id)[n];
      if (!attempt) {
        attempt = {
          provider: provider.id,
          id: randomUUID(),
          rawId: randomUUID(),
          assessmentId: randomUUID(),
        };
        saved.attempts.push(attempt);
        await save();
      }
      if (!attempt.assessment) {
        await check(provider);
        if (!attempt.reservationId) {
          const reserved = await host.request(
            {
              action: "reserve",
              fence,
              final_result: false,
              maximum: provider.maximum,
              provider_attempt: attempt.id,
            },
            context.abortSignal,
          );
          if (reserved.kind !== "reservation")
            throw new Error("Host did not reserve judgement usage");
          attempt.reservationId = reserved.reservation.id;
          await save();
        }
        if (!attempt.reply) {
          if (attempt.started) {
            attempt.reply = {
              status: 503,
              raw: {
                failure:
                  "Provider outcome lost during interruption; usage remains unknown",
              },
              usage: {},
            };
          } else {
            attempt.started = true;
            await save();
            await check(provider);
            const signal = AbortSignal.any([
              AbortSignal.timeout(
                Math.max(1, Date.parse(packet.deadline) - Date.now()),
              ),
              ...(context.abortSignal ? [context.abortSignal] : []),
            ]);
            try {
              attempt.reply = await provider.evaluate(packet, signal);
            } catch {
              attempt.reply = {
                status: 503,
                raw: {
                  failure: "Provider request failed or exceeded its deadline",
                },
                usage: {},
              };
            }
          }
          await save();
        }
        const reply = attempt.reply;
        let assessment: SemanticAssessment = {
          id: attempt.assessmentId,
          packet_id: packet.id,
          raw_artifact_id: attempt.rawId,
          reservation_id: attempt.reservationId,
          provider: provider.id,
          requested_model: provider.model,
          status: reply.status === 200 ? "invalid_response" : "unavailable",
          answers: {},
          failure: `Provider status ${reply.status}`,
          usage: reply.usage,
          assessed_at: new Date().toISOString(),
          expires_at: packet.expires_at,
        };
        if (reply.status === 200) {
          try {
            const normalized = validateResponse(
              packet,
              reply.raw,
              provider.distributions,
            );
            assessment = {
              ...assessment,
              ...normalized,
              model_release:
                normalized.returned_model === provider.release
                  ? provider.release
                  : undefined,
              status: "answered",
              failure: undefined,
            };
          } catch (error) {
            assessment.failure =
              error instanceof Error
                ? error.message
                : "Invalid provider response";
          }
        }
        if (attempt.prepared) assessment = attempt.prepared;
        else {
          attempt.prepared = assessment;
          await save();
        }
        // A lost/unknown bill continues to hold the maximum reservation. Known
        // token counts remain available in the assessment without inventing cost.
        const u = reply.usage;
        const observed =
          u.input_tokens != null &&
          u.output_tokens != null &&
          u.cost_microunits != null
            ? {
                ...provider.maximum,
                tokens: u.input_tokens + u.output_tokens,
                cost_microunits: u.cost_microunits,
                output_bytes: 0,
                sandbox_time_ms: 0,
                sandbox_cpu_ms: 0,
              }
            : null;
        await host.request(
          {
            action: "settle_usage",
            fence,
            reservation_id: attempt.reservationId,
            observed,
          },
          context.abortSignal,
        );
        await check(provider);
        const rawText = JSON.stringify({
          status: reply.status,
          response: reply.raw,
        });
        if (Buffer.byteLength(rawText) > 65536)
          throw new Error(
            "Raw judgement response exceeds artifact limit; no assessment adopted",
          );
        await host.request(
          {
            action: "publish_artifact",
            fence,
            request_id: attempt.rawId,
            label: "Raw judgement response",
            text: rawText,
            dependencies: [packet.id],
          },
          context.abortSignal,
        );
        const result = await host.request(
          { action: "record_assessment", fence, assessment },
          context.abortSignal,
        );
        if (result.kind !== "assessment" || !result.assessment)
          throw new Error("Host did not record assessment");
        attempt.assessment = result.assessment;
        await save();
      }
      if (![429, 529].includes(attempt.reply!.status) || n + 1 >= limit)
        return attempt.assessment;
      const retryAfter = attempt.reply!.retryAfterMs;
      const wait = Math.max(
        0,
        retryAfter !== undefined && Number.isFinite(retryAfter)
          ? retryAfter
          : 100 * 2 ** n,
      );
      // Do not shorten a server-requested delay just to fit our retry window.
      if (wait > 2000) return attempt.assessment;
      if (Date.now() + wait >= Date.parse(packet.deadline))
        return attempt.assessment;
      await delay(wait, undefined, { signal: context.abortSignal });
    }
    throw new Error("No judgement attempt completed");
  }
  async function checkQualificationEvidence() {
    for (const gate of options.qualifications ?? []) {
      if (packet.questions.some((q) => q.definition.id === gate.family)) {
        await host.request(
          {
            action: "read_input",
            fence,
            artifact_id: gate.evaluationArtifact,
            offset: 0,
            limit: 65536,
          },
          context.abortSignal,
        );
      }
    }
  }
  // Re-read access even when recovering an already recorded decision.
  await check(options.reference);
  if (saved.decision) {
    if (mode === "qualified") await checkQualificationEvidence();
    const checked = await host.request(
      { action: "record_judgement_decision", fence, decision: saved.decision },
      context.abortSignal,
    );
    if (checked.kind !== "judgement_decision")
      throw new Error("Saved decision is no longer available");
    return checked.decision;
  }
  const assessments: SemanticAssessment[] = [];
  let selected: SemanticAssessment | undefined;
  if (mode !== "reference" && options.jev) {
    const jev = await assess(options.jev);
    assessments.push(jev);
    if (
      mode === "qualified" &&
      qualified(packet, jev, options.qualifications ?? []) &&
      inconsistencies(jev, rules).length === 0
    ) {
      await checkQualificationEvidence();
      selected = jev;
    }
  }
  if (!selected) {
    const reference = await assess(options.reference);
    assessments.push(reference);
    if (
      reference.status === "answered" &&
      inconsistencies(reference, rules).length === 0
    )
      selected = reference;
  }
  const all = [
    ...new Map(
      [
        ...saved.attempts.flatMap((a) => (a.assessment ? [a.assessment] : [])),
        ...assessments,
      ].map((a) => [a.id, a]),
    ).values(),
  ];
  const decision = decide(packet, saved.decisionId, all, selected, mode);
  decision.inconsistencies.push(
    ...all.flatMap((a) =>
      inconsistencies(a, rules).map((rule) => `${a.provider}: ${rule}`),
    ),
  );
  const recorded = await host.request(
    { action: "record_judgement_decision", fence, decision },
    context.abortSignal,
  );
  if (recorded.kind !== "judgement_decision")
    throw new Error("Host did not record policy decision");
  saved.decision = recorded.decision;
  await save();
  return recorded.decision;
}
