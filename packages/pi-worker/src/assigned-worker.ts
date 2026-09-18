import { reconcileActiveWork } from "../../maintenance/src/workspace.js";
import {
  prepareInvestigation,
  investigationWorkspace,
  preserveChildEvidence,
} from "./scoped-operation.js";
import { briefAccess, workspaceFromBrief } from "./workspace.js";
import { randomUUID } from "node:crypto";
import {
  BACKGROUND_CONTEXT,
  withCancel,
  value,
  operationState,
  type Context,
  type JsonValue,
  type AgentToolResult,
} from "@earendil-works/pi-agent-core";
import type {
  Assignment,
  WorkResult,
} from "../../../contracts/generated/assignment.js";
import type {
  Fence,
  Resources,
} from "../../../contracts/generated/host-request.js";
import type { HostCommands } from "./host-client.js";
import { createWorkerHarness, type WorkerOptions } from "./harness.js";

export interface WorkerProfile {
  id: string;
  revision: number;
  maxPayloadBytes: number;
  maxOutputTokens: number;
  providerReservation: Resources;
  /** Operator-reviewed providers which retain no request content. Empty denies restricted work. */
  restrictedProviders?: string[];
}
export interface ExecutionLifecycle {
  renew(assignment: Assignment): Promise<void>;
  finish(cancelled: boolean): Promise<void>;
}
interface PendingUsage {
  reservationId: string;
  observed: Resources | null;
}
interface SavedResult {
  requestId: string;
  result: WorkResult;
}

/** One assignment, one attached Pi session, and one host-mapped operation. */
export async function attachAssignedWorker(
  assignment: Assignment,
  host: HostCommands,
  profile: WorkerProfile,
  options: WorkerOptions,
  parent: Context = BACKGROUND_CONTEXT,
  lifecycle?: ExecutionLifecycle,
) {
  const { job } = assignment;
  const reference = job.spec.brief.profile;
  if (reference.id !== profile.id || reference.revision !== profile.revision)
    throw new Error("Assignment profile is unavailable");
  if (options.session.metadata.id !== job.session_id)
    throw new Error("Assignment session mismatch");
  if (
    options.tools.some(
      (name) => !job.spec.brief.capabilities.tools.includes(name),
    )
  )
    throw new Error("Tool is outside the assignment capability set");
  if (
    profile.maxPayloadBytes <= 0 ||
    profile.maxOutputTokens <= 0 ||
    profile.providerReservation.provider_calls !== 1
  )
    throw new Error("Invalid provider limits");
  const fence: Fence = {
    job_id: job.id,
    owner_id: assignment.owner_id,
    epoch: assignment.epoch,
  };
  const { context, cancel } = withCancel(parent);
  let expiresAt = Date.parse(assignment.expires_at);
  let cancelled = job.cancel_requested;
  let lost: Error | undefined;
  let closed = false;
  let driving = false;
  let expiryTimer: ReturnType<typeof setTimeout>;
  function armExpiry() {
    clearTimeout(expiryTimer);
    expiryTimer = setTimeout(
      () => {
        lost = new Error("Assignment lease or deadline expired");
        cancel();
      },
      Math.max(0, Math.min(expiresAt, Date.parse(job.deadline)) - Date.now()),
    );
    expiryTimer.unref();
  }
  armExpiry();
  function active() {
    if (lost) throw lost;
    if (
      closed ||
      context.abortSignal?.aborted ||
      Date.now() >= expiresAt ||
      Date.now() >= Date.parse(job.deadline)
    )
      throw new Error("Assignment is no longer active");
  }
  const pendingAddress = value<PendingUsage>(
    "memory.provider",
    job.operation_id,
  );
  const resultAddress = value<SavedResult>("memory.result", job.operation_id);
  async function flushUsage() {
    const pending = (await options.session.getValue(pendingAddress, context))
      ?.value;
    if (!pending) return;
    await host.request(
      {
        action: "settle_usage",
        fence,
        reservation_id: pending.reservationId,
        observed: pending.observed,
      },
      context.abortSignal,
    );
    await options.session.deleteValue(pendingAddress, context);
  }
  const created = await createWorkerHarness(
    {
      ...options,
      workspace: {
        ...options.workspace,
        initial:
          options.workspace?.initial ??
          workspaceFromBrief(
            job.spec.brief,
            job.deadline,
            job.spec.retain_until,
          ),
        profile: job.spec.brief.profile,
        maxPayloadBytes: Math.min(
          profile.maxPayloadBytes,
          options.workspace?.maxPayloadBytes ?? profile.maxPayloadBytes,
        ),
        refresh: async (state) => {
          active();
          const result = await reconcileActiveWork(
            host,
            assignment,
            state,
            state.change_cursor ?? 0,
            context.abortSignal,
          );
          const refreshed = {
            ...result.workspace,
            change_cursor: result.cursor,
          };
          return options.workspace?.refresh
            ? options.workspace.refresh(refreshed)
            : refreshed;
        },
        onManifest: async (manifest) => {
          await host.request({ action: "record_context", fence, manifest }, context.abortSignal);
          await options.workspace?.onManifest?.(manifest);
        },
        readAccess: async () => {
          active();
          const checked = await host.request(
            { action: "inspect_assignment", fence },
            context.abortSignal,
          );
          if (
            checked.kind !== "assignment" ||
            checked.assignment.job.cancel_requested
          )
            throw new Error("Workspace assignment is no longer active");
          const grant = briefAccess(checked.assignment.job.spec.brief);
          const narrower = await options.workspace?.readAccess();
          return narrower
            ? {
                revision: JSON.stringify([grant.revision, narrower.revision]),
                allows: (scope, inputs) =>
                  grant.allows(scope, inputs) && narrower.allows(scope, inputs),
              }
            : grant;
        },
      },
      wrapTool(tool) {
        const wrapped = options.wrapTool ? options.wrapTool(tool) : tool;
        // Python owns its execution receipt and sandbox resource reservation.
        if (tool.name === "python") return wrapped;
        return {
          ...wrapped,
          replay: tool.name === "read" ? "safe" : "never",
          async execute(
            toolCallId,
            params,
            update,
            toolContext,
            invocation,
            toolExecutionContext,
          ) {
            active();
            if (cancelled) throw new Error("Job cancellation requested");

            const prepared = await host.request(
              {
                action: "prepare_effect",
                fence,
                request: {
                  logical_operation_id: `${job.operation_id}/${invocation.invocationId}`,
                  invocation_id: invocation.invocationId,
                  kind: tool.name,
                  replay: tool.name === "read" ? "observation" : "reconcile",
                  arguments: params,
                },
              },
              context.abortSignal,
            );
            if (prepared.kind !== "effect")
              throw new Error("Host did not prepare the tool effect");
            const effect = prepared.effect;
            if (effect.state === "succeeded" && effect.receipt)
              return effect.receipt as unknown as AgentToolResult<unknown>;
            await host.request(
              { action: "begin_effect", fence, effect_id: effect.id },
              context.abortSignal,
            );
            // A thrown transport error cannot establish whether a remote mutation happened.
            let result: AgentToolResult<unknown>;
            try {
              result = await wrapped.execute(
                toolCallId,
                params,
                update,
                toolContext,
                invocation,
                toolExecutionContext,
              );
            } catch (error) {
              await host.request({
                action: "report_effect",
                fence,
                effect_id: effect.id,
                state: "outcome_unknown",
                receipt: { reason: "Tool execution did not return a receipt" },
              });
              throw error;
            }
            await host.request(
              {
                action: "report_effect",
                fence,
                effect_id: effect.id,
                state: "succeeded",
                receipt: result as unknown as JsonValue,
              },
              context.abortSignal,
            );
            return result;
          },
        };
      },
    },
    context,
  );
  const { harness } = created;
  const lane = await harness.lane("main", context);
  const other = created.open.find(
    (operation) => operation.operationId !== job.operation_id,
  );
  if (other) {
    await harness.close(parent);
    throw new Error("Session contains work outside this assignment");
  }
  harness.hooks.on(
    "before_request",
    async ({ runId, attempt, step, model }) => {
      active();
      if (cancelled) throw new Error("Job cancellation requested");
      if (job.spec.brief.disclosure_policy === "restricted" && !profile.restrictedProviders?.includes(model.provider))
        throw new Error("Provider retention is not approved for restricted content");
      if (
        model.id !== options.model.id ||
        model.provider !== options.model.provider ||
        model.maxTokens > profile.maxOutputTokens
      )
        throw new Error("Model exceeds the assigned profile");
      if (
        profile.providerReservation.tokens <
        profile.maxPayloadBytes + model.maxTokens
      )
        throw new Error(
          "Provider reservation cannot cover the bounded request",
        );
      await flushUsage();
      const state = (
        await options.session.getValue(operationState(runId), context)
      )?.value;
      if (!state)
        throw new Error("Provider call has no persisted Pi operation");
      const stepId =
        "generationContext" in state
          ? state.generationContext.stepId
          : "task" in state
            ? state.task.taskId
            : "stepId" in state
              ? `${state.stepId}/${state.poll}`
              : undefined;
      if (!stepId) throw new Error(`Unsupported provider state: ${state.at}`);
      const reservation = await host.request(
        {
          action: "reserve",
          fence,
          provider_attempt: `${runId}/${step}/${stepId}/${attempt}/${assignment.epoch}/${randomUUID()}`,
          maximum: profile.providerReservation,
          final_result: false,
        },
        context.abortSignal,
      );
      if (reservation.kind !== "reservation")
        throw new Error("Host did not reserve provider usage");
      await options.session.setValue(
        pendingAddress,
        { reservationId: reservation.reservation.id, observed: null },
        context,
      );
      return {
        streamOptions: {
          maxRetries: 0,
          timeoutMs: Math.max(
            1,
            Math.min(expiresAt, Date.parse(job.deadline)) - Date.now(),
          ),
        },
      };
    },
  );
  harness.hooks.on("before_payload", async ({ payload }) => {
    active();
    if (Buffer.byteLength(JSON.stringify(payload)) > profile.maxPayloadBytes)
      throw new Error("Provider payload exceeds the profile limit");
    // Check at final egress, after rendering and reservation. A revoked job cannot
    // send its already-rendered context on a subsequent provider request.
    await host.request({ action: "check_provider", fence, provider: options.model.provider }, context.abortSignal);
    return undefined;
  });
  harness.hooks.on("after_response", async ({ message }) => {
    const pending = (await options.session.getValue(pendingAddress, context))
      ?.value;
    if (!pending) throw new Error("Provider response has no reservation");
    const usage = message.usage;
    await options.session.setValue(
      pendingAddress,
      {
        ...pending,
        observed:
          message.stopReason === "error" || message.stopReason === "aborted"
            ? null
            : {
                tokens: usage.totalTokens,
                provider_calls: 1,
                cost_microunits: Math.ceil(usage.cost.total * 1_000_000),
                sandbox_cpu_ms: 0,
                sandbox_time_ms: 0,
                output_bytes: 0,
              },
      },
      context,
    );
    await flushUsage();
    return undefined;
  });

  async function renew(updateEndpoint = true) {
    active();
    try {
      const response = await host.request(
        { action: "renew", fence, lease_seconds: 30 },
        context.abortSignal,
      );
      if (response.kind !== "assignment")
        throw new Error("Host did not renew assignment");
      expiresAt = Date.parse(response.assignment.expires_at);
      armExpiry();
      if (updateEndpoint) await lifecycle?.renew(response.assignment);
      cancelled = response.assignment.job.cancel_requested;
      if (cancelled && updateEndpoint)
        await lane.requestAbort(job.operation_id, context);
    } catch (error) {
      lost =
        error instanceof Error ? error : new Error("Assignment renewal failed");
      cancel();
      throw lost;
    }
  }
  async function resultFor(outcome: {
    status: string;
    fromTipId: string | null;
    tipId: string | null;
  }): Promise<WorkResult> {
    const entries = await lane.findEntries({ order: "newestFirst" }, context);
    const currentEntries = [];
    for (const entry of entries) {
      if (entry.id === outcome.fromTipId) break;
      currentEntries.push(entry);
    }
    if (outcome.status === "completed") {
      const answer = currentEntries.find(
        (e) => e.type === "message" && e.message.role === "assistant",
      );
      if (answer?.type === "message" && answer.message.role === "assistant") {
        const text = answer.message.content
          .filter((p) => p.type === "text")
          .map((p) => p.text)
          .join("\n");
        try {
          const parsed: unknown = JSON.parse(text);
          // The host performs full schema and scope validation before committing.
          if (
            parsed &&
            typeof parsed === "object" &&
            "schema_version" in parsed &&
            parsed.schema_version === "1" &&
            "status" in parsed &&
            ["complete", "partial", "blocked"].includes(String(parsed.status))
          )
            return parsed as WorkResult;
        } catch {
          /* Report the missing application result below. */
        }
      }
    }
    return {
      schema_version: "1",
      status: "partial",
      examined_scope: job.spec.brief.scope,
      inputs: job.spec.brief.inputs,
      findings: [],
      coverage: {
        examined: [],
        unexamined: ["The assignment did not produce a validated work result"],
      },
      unresolved_work: [
        `Pi operation ${outcome.status}; application work remains unresolved`,
      ],
      proposed_changes: [],
      child_outputs: [],
      known_effects: [],
      usage: {
        status: "unknown",
        input_tokens: null,
        output_tokens: null,
        cost: null,
      },
    };
  }
  return {
    harness,
    workspace: created.workspace!,
    lane,
    renew: () => renew(),
    /** Watch begins with a snapshot; reconnect obtains a fresh snapshot from Pi. */
    watch: () => lane.watch(context),
    async run() {
      if (driving) throw new Error("Assignment is already being driven");
      driving = true;
      let heartbeat: ReturnType<typeof setTimeout> | undefined;
      let renewal: Promise<void> = Promise.resolve();
      let stopped = false;
      let finishing = false;
      const schedule = () => {
        if (stopped) return;
        heartbeat = setTimeout(() => {
          renewal = renew(!finishing)
            .then(schedule)
            .catch(() => {});
        }, 5_000);
      };
      const finishExecution = async (cancelled: boolean) => {
        // Exports can outlast a lease. Keep host ownership alive while the
        // endpoint is fenced; renewing that endpoint would reopen execution.
        finishing = true;
        if (heartbeat) clearTimeout(heartbeat);
        await renewal;
        if (heartbeat) clearTimeout(heartbeat);
        try {
          if (lifecycle) {
            await renew(false);
            schedule();
            await lifecycle.finish(cancelled);
          }
        } finally {
          stopped = true;
          if (heartbeat) clearTimeout(heartbeat);
          await renewal;
        }
      };
      try {
        const snapshot = await host.request({
          action: "inspect_job",
          job_id: job.id,
        });
        if (snapshot.kind !== "job")
          throw new Error("Host did not return assignment state");
        if (snapshot.job.result)
          return { kind: "settled" as const, job: snapshot.job };
        active();
        cancelled = snapshot.job.cancel_requested;
        if (cancelled) {
          const execution = await lane.inspectExecution(context);
          if (execution.current) {
            if (execution.current.id !== job.operation_id)
              throw new Error("Pi operation does not match cancellation");
            await lane.requestAbort(job.operation_id, context);
            await lane.drive(
              {
                operationId: job.operation_id,
                waitForRetry: false,
                pollDeferred: false,
              },
              context,
            );
          }
          await flushUsage();
          await finishExecution(true);
          await host.request(
            { action: "acknowledge_cancellation", fence },
            context.abortSignal,
          );
          return { kind: "cancelled" as const };
        }
        await host.request({ action: "start", fence }, context.abortSignal);
        schedule();
        await flushUsage();
        let investigation:
          Awaited<ReturnType<typeof prepareInvestigation>> | undefined;
        if (options.investigation) {
          investigation = await prepareInvestigation(
            assignment,
            host,
            options.session,
            options.investigation,
            context,
          );
          if (investigation.kind === "waiting")
            return { kind: "waiting" as const };
          const state = await created.workspace!.state();
          const enriched = investigationWorkspace(
            state,
            investigation.children,
            investigation.bundleId,
          );
          if (enriched.entries.length !== state.entries.length)
            await created.workspace!.checkpoint(enriched);
        }
        let saved = (await options.session.getValue(resultAddress, context))
          ?.value;
        if (!saved) {
          let outcome = await lane.getResult(job.operation_id, context);
          if (!outcome) {
            const execution = await lane.inspectExecution(context);
            if (execution.current && execution.current.id !== job.operation_id)
              throw new Error("Pi operation does not match the assignment");
            if (!execution.current) {
              const accepted = await lane.accept(
                {
                  kind: "prompt",
                  operationId: job.operation_id,
                  prompt:
                    "Carry out the assigned goal using the supplied workspace. Return the application WorkResult and state any unexamined evidence.",
                },
                context,
              );
              if (!accepted.ok)
                throw new Error(
                  `Pi admission failed: ${JSON.stringify(accepted.error)}`,
                );
            }
            if (cancelled) await lane.requestAbort(job.operation_id, context);
            const persisted = (
              await options.session.getValue(
                operationState(job.operation_id),
                context,
              )
            )?.value;
            const driven = await lane.drive(
              {
                operationId: job.operation_id,
                waitForRetry: false,
                pollDeferred: persisted?.at.startsWith("deferred.") ?? false,
              },
              context,
            );
            if (!driven.ok)
              throw new Error(
                `Pi drive failed: ${JSON.stringify(driven.error)}`,
              );
            if (driven.value.kind === "waiting") {
              const readyAt =
                driven.value.reason === "retry"
                  ? driven.value.notBefore
                  : Date.now() + 5_000;
              await host.request(
                {
                  action: "wait",
                  fence,
                  reason: `pi_${driven.value.reason}`,
                  ready_at: new Date(
                    Math.min(
                      Math.max(readyAt, Date.now() + 100),
                      Date.parse(job.deadline) - 1,
                    ),
                  ).toISOString(),
                },
                context.abortSignal,
              );
              return { kind: "waiting" as const };
            }
            outcome = driven.value.outcome;
          }
          if (cancelled) {
            await finishExecution(true);
            await host.request(
              { action: "acknowledge_cancellation", fence },
              context.abortSignal,
            );
            return { kind: "cancelled" as const };
          }
          await flushUsage();
          let result = await resultFor(outcome);
          if (investigation?.kind === "ready")
            result = preserveChildEvidence(
              result,
              investigation.children,
              investigation.bundleId,
              job.spec.brief,
            );
          // Billing authority is the host reservation ledger, not generated text.
          result.usage = {
            status: "unknown",
            input_tokens: null,
            output_tokens: null,
            cost: null,
          };
          saved = { requestId: randomUUID(), result };
          await options.session.setValue(resultAddress, saved, context);
        }
        const entries = await lane.findEntries({ type: "custom" }, context);
        if (
          !entries.some(
            (entry) =>
              entry.type === "custom" &&
              entry.customType === "memory.result" &&
              entry.data &&
              typeof entry.data === "object" &&
              !Array.isArray(entry.data) &&
              entry.data.operationId === job.operation_id,
          )
        ) {
          await lane.appendCustomEntry(
            "memory.result",
            {
              operationId: job.operation_id,
              status: saved.result.status,
              requestId: saved.requestId,
            },
            context,
          );
        }
        await finishExecution(false);
        active();
        const committed = await host.request(
          {
            action: "complete",
            request_id: saved.requestId,
            fence,
            result: saved.result,
          },
          context.abortSignal,
        );
        if (committed.kind !== "job")
          throw new Error("Host did not return the committed result");
        return { kind: "settled" as const, job: committed.job };
      } finally {
        stopped = true;
        if (heartbeat) clearTimeout(heartbeat);
        await renewal;
        driving = false;
      }
    },
    async close() {
      closed = true;
      clearTimeout(expiryTimer);
      cancel();
      await harness.close(parent);
    },
  };
}
