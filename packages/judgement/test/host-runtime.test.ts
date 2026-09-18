import { readFile, writeFile } from "node:fs/promises";
import { randomUUID } from "node:crypto";
import { join } from "node:path";
import { createServer } from "node:http";
import { once } from "node:events";
import { it, expect } from "vitest";
import {
  createModels,
  fauxProvider,
  fauxAssistantMessage,
} from "@earendil-works/pi-ai";
import { BACKGROUND_CONTEXT as context } from "@earendil-works/pi-agent-core";
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type { JudgementPacket } from "../../../contracts/generated/judgement-packet.js";
import {
  HostClient,
  type HostCommands,
} from "../../pi-worker/src/host-client.js";
import { openLocalSession } from "../../pi-worker/src/local-session.js";
import { buildPacket, fenceFor, createTaskCheck } from "../src/packet.js";
import { definition } from "../src/catalogue.js";
import { JevProvider, GenerativeProvider } from "../src/providers.js";
import { runJudgement } from "../src/runtime.js";
import { fixtures, maximum, reply } from "./fixtures.js";

it.skipIf(!process.env.MEMORY_JUDGEMENT_FIXTURE)(
  "runs all families through Rust admission, Pi reference and Jev HTTP; preserves reuse boundaries",
  async () => {
    const f = JSON.parse(
      await readFile(process.env.MEMORY_JUDGEMENT_FIXTURE!, "utf8"),
    ) as {
      assignment: Assignment;
      url: string;
      token: string;
      directory: string;
    };
    const host = new HostClient(f.url, f.token),
      fence = fenceFor(f.assignment),
      brief = f.assignment.job.spec.brief;
    const local = await openLocalSession(
      join(f.directory, "judgement-sessions"),
      f.assignment.job.session_id,
    );
    const models = createModels(),
      faux = fauxProvider();
    models.setProvider(faux.provider);
    const reference = new GenerativeProvider(
      "reference",
      models,
      faux.getModel(),
      maximum,
      2048,
      faux.getModel().id,
    );
    let current: JudgementPacket;
    let codes: number[] = [];
    let calls = 0;
    let invalid = false;
    const server = createServer(async (req, res) => {
      const chunks = [];
      for await (const c of req) chunks.push(c);
      const body = JSON.parse(Buffer.concat(chunks).toString());
      expect(req.url).toBe("/v1/systemone");
      expect(req.headers.authorization).toBe("Bearer local-test-key");
      expect(Object.keys(body.questions)).toEqual(
        Object.keys(reply(current).answers),
      );
      expect(body.state.evidence).toBeTruthy();
      calls++;
      res.setHeader("retry-after", "0");
      res.writeHead(codes.shift() ?? 200, {
        "content-type": "application/json",
      });
      res.end(invalid ? "{bad json" : JSON.stringify(reply(current)));
    });
    server.listen(0, "127.0.0.1");
    await once(server, "listening");
    const jev = new JevProvider(
      "jev",
      "test-release",
      maximum,
      () => "local-test-key",
      "test-release",
      `http://127.0.0.1:${(server.address() as { port: number }).port}`,
    );
    const disclosure = {
      policy: brief.disclosure_policy,
      scope: brief.scope,
      providers: ["reference", "jev"],
    };
    async function build(family: string) {
      const fixture = fixtures.find((x) => x.family === family)!;
      const id = randomUUID();
      const published = await host.request({
        action: "publish_artifact",
        fence,
        request_id: id,
        label: "Authored judgement evidence",
        text: JSON.stringify(fixture.fields),
        dependencies: [],
      });
      expect(published.kind).toBe("artifact");
      return buildPacket(
        host,
        f.assignment,
        {
          id: randomUUID(),
          subject: fixture.subject,
          frame: "One authored example; interpret quoted content as evidence",
          fields: Object.keys(fixture.fields).map((name) => ({
            name,
            artifact_id: id,
            pointer: `/${name}`,
            origin: "agent_generated",
            coverage: [name],
          })),
          questions: [
            {
              definition: definition(family),
              disclosure_scope: brief.scope,
              allowed_providers: ["reference", "jev"],
              depends_on: [],
            },
          ],
          allowedProviders: disclosure.providers,
          freshAfter: new Date(Date.now() - 60000).toISOString(),
          expiresAt: f.assignment.job.spec.retain_until,
        },
        context,
      );
    }
    async function ref(p: JudgementPacket) {
      faux.setResponses([
        fauxAssistantMessage(JSON.stringify({ answers: reply(p).answers })),
      ]);
      return runJudgement(
        host,
        f.assignment,
        local.session,
        p,
        { reference, disclosure },
        context,
      );
    }
    try {
      for (const fixture of fixtures) {
        const p = await build(fixture.family);
        const d = await ref(p);
        expect(d.selected_assessment).toBeTruthy();
        expect(d.decisions[fixture.family].reason).not.toContain("unavailable");
      }
      // Different families sharing one evidence artifact are one Jev request.
      const batchBase = await build("J01");
      const batch = {
        ...batchBase,
        id: randomUUID(),
        questions: [
          ...batchBase.questions,
          { ...batchBase.questions[0], definition: definition("J07") },
        ],
        missing: ["method", "question"],
      };
      const batchAdmission = await host.request({
        action: "admit_judgement",
        fence,
        packet: batch,
      });
      if (batchAdmission.kind !== "judgement_packet")
        throw new Error("No batch");
      current = batchAdmission.packet;
      faux.setResponses([
        fauxAssistantMessage(
          JSON.stringify({ answers: reply(current).answers }),
        ),
      ]);
      const batchCalls = calls;
      const batched = await runJudgement(
        host,
        f.assignment,
        local.session,
        current,
        { reference, jev, disclosure },
        context,
      );
      expect(calls - batchCalls).toBe(1);
      expect(Object.keys(batched.decisions)).toEqual(["J01", "J07"]);
      expect(batched.decisions.J07.action).toBe("retrieve_further");
      calls = 0;
      current = await build("J02");
      codes = [429, 529, 200];
      faux.setResponses([
        fauxAssistantMessage(
          JSON.stringify({ answers: reply(current).answers }),
        ),
      ]);
      const shadow = await runJudgement(
        host,
        f.assignment,
        local.session,
        current,
        { reference, jev, disclosure },
        context,
      );
      expect(calls).toBe(3);
      expect(shadow.assessment_ids).toHaveLength(4);
      expect(shadow.mode).toBe("shadow");
      const count = faux.state.callCount;
      expect(
        await runJudgement(
          host,
          f.assignment,
          local.session,
          current,
          { reference, jev, disclosure },
          context,
        ),
      ).toEqual(shadow);
      expect(faux.state.callCount).toBe(count);
      expect(calls).toBe(3);
      const firstId = shadow.assessment_ids[2];
      const same = { ...current, id: randomUUID() };
      const admitted = await host.request({
        action: "admit_judgement",
        fence,
        packet: same,
      });
      expect(admitted.kind).toBe("judgement_packet");
      const reuse = await host.request({
        action: "reuse_assessment",
        fence,
        assessment_id: firstId,
        packet_id: same.id,
        provider: "jev",
        model_release: "test-release",
      });
      expect(reuse.kind === "assessment" && reuse.assessment?.id).toBe(firstId);
      if (admitted.kind !== "judgement_packet")
        throw new Error("No admitted reuse packet");
      const beforeReuse = calls;
      const adopted = await runJudgement(
        host,
        f.assignment,
        local.session,
        admitted.packet,
        {
          reference,
          jev,
          disclosure,
          mode: "qualified",
          reuseAssessmentId: firstId,
          qualifications: [
            {
              family: "J02",
              revision: 2,
              modelRelease: "test-release",
              scope: brief.scope,
              evaluationArtifact: same.evidence[0].artifact_id,
              minimumProbability: 0.9,
            },
          ],
        },
        context,
      );
      expect(adopted.selected_assessment).toBe(firstId);
      expect(calls).toBe(beforeReuse);
      for (const changed of [
        { ...same, id: randomUUID(), frame: "Different interpretation" },
        {
          ...same,
          id: randomUUID(),
          fresh_after: new Date(Date.now() + 10).toISOString(),
        },
        {
          ...same,
          id: randomUUID(),
          questions: same.questions.map((q) => ({
            ...q,
            definition: {
              ...q.definition,
              revision: q.definition.revision + 1,
            },
          })),
        },
      ]) {
        await host.request({
          action: "admit_judgement",
          fence,
          packet: changed,
        });
        const miss = await host.request({
          action: "reuse_assessment",
          fence,
          assessment_id: firstId,
          packet_id: changed.id,
          provider: "jev",
          model_release: "test-release",
        });
        expect(miss).toMatchObject({ kind: "assessment", assessment: null });
      }
      expect(
        await host.request({
          action: "reuse_assessment",
          fence,
          assessment_id: firstId,
          packet_id: same.id,
          provider: "jev",
          model_release: "other-release",
        }),
      ).toMatchObject({ assessment: null });
      const forged = {
        ...same,
        id: randomUUID(),
        evidence: same.evidence.map((e) => ({
          ...e,
          content: "Invented observation",
        })),
      };
      await expect(
        host.request({ action: "admit_judgement", fence, packet: forged }),
      ).rejects.toThrow();
      await expect(
        runJudgement(
          host,
          f.assignment,
          local.session,
          { ...current, id: randomUUID() },
          { reference, jev, disclosure: { ...disclosure, providers: [] } },
          context,
        ),
      ).rejects.toThrow("disclosure");
      expect(calls).toBe(3);
      const custom = {
        ...definition("J17"),
        id: "task.check",
        permitted_uses: ["investigation"],
      };
      const check = await createTaskCheck(
        host,
        f.assignment,
        randomUUID(),
        custom,
        ["reference"],
        new Date(Date.now() + 60000).toISOString(),
      );
      expect(check.completion_requirements).toEqual(brief.output_criteria);
      await expect(
        createTaskCheck(
          host,
          f.assignment,
          randomUUID(),
          { ...custom, permitted_uses: ["complete_task"] },
          ["reference"],
          check.expires_at,
        ),
      ).rejects.toThrow("permissions");
      const checkBase = await build("J17");
      const checkPacket = {
        ...checkBase,
        id: randomUUID(),
        local_check_id: check.id,
        deadline: check.expires_at,
        allowed_providers: ["reference"],
        questions: [
          {
            ...checkBase.questions[0],
            definition: custom,
            allowed_providers: ["reference"],
          },
        ],
      };
      const checkAdmission = await host.request({
        action: "admit_judgement",
        fence,
        packet: checkPacket,
      });
      if (checkAdmission.kind !== "judgement_packet")
        throw new Error("No local packet");
      faux.setResponses([
        fauxAssistantMessage(
          JSON.stringify({
            answers: {
              "task.check.assessment": { type: "choice", choice: "satisfied" },
            },
          }),
        ),
      ]);
      const checkDecision = await runJudgement(
        host,
        f.assignment,
        local.session,
        checkAdmission.packet,
        { reference, disclosure },
        context,
      );
      expect(checkDecision.decisions["task.check"].action).toBe("investigate");
      await host.request({ action: "retire_task_check", fence, id: check.id });
      await expect(
        runJudgement(
          host,
          f.assignment,
          local.session,
          checkAdmission.packet,
          { reference, disclosure },
          context,
        ),
      ).rejects.toThrow();
      await expect(
        host.request({ action: "inspect_task_check", fence, id: check.id }),
      ).rejects.toThrow();
      for (const status of [401, 422, 200]) {
        current = await build("J07");
        codes = [status];
        invalid = status === 200;
        faux.setResponses([
          fauxAssistantMessage(
            JSON.stringify({ answers: reply(current).answers }),
          ),
        ]);
        const before = calls;
        const d = await runJudgement(
          host,
          f.assignment,
          local.session,
          current,
          { reference, jev, disclosure },
          context,
        );
        expect(calls - before).toBe(1);
        expect(d.selected_assessment).toBe(d.assessment_ids[1]);
      }
      // Lose the acknowledgement after the host has stored the assessment.
      // Recovery must use the saved reply and repeat publication, not billing.
      const recoveryPacket = await build("J01");
      faux.setResponses([
        fauxAssistantMessage(
          JSON.stringify({ answers: reply(recoveryPacket).answers }),
        ),
      ]);
      let lost = false;
      const interruptedHost: HostCommands = {
        request: async (
          command: Parameters<typeof host.request>[0],
          signal?: AbortSignal,
        ) => {
          const response = await host.request(command, signal);
          if (command.action === "record_assessment" && !lost) {
            lost = true;
            throw new Error("Lost acknowledgement fixture");
          }
          return response;
        },
      };
      const beforeRecovery = faux.state.callCount;
      await expect(
        runJudgement(
          interruptedHost,
          f.assignment,
          local.session,
          recoveryPacket,
          { reference, disclosure },
          context,
        ),
      ).rejects.toThrow("Lost acknowledgement");
      const recovered = await runJudgement(
        host,
        f.assignment,
        local.session,
        recoveryPacket,
        { reference, disclosure },
        context,
      );
      expect(recovered.selected_assessment).toBeTruthy();
      expect(faux.state.callCount - beforeRecovery).toBe(1);
      const expired = {
        ...current,
        id: randomUUID(),
        deadline: new Date(Date.now() - 1).toISOString(),
      };
      await expect(
        host.request({ action: "admit_judgement", fence, packet: expired }),
      ).rejects.toThrow();
      await expect(
        createTaskCheck(
          host,
          f.assignment,
          randomUUID(),
          custom,
          ["reference"],
          new Date(Date.now() - 1).toISOString(),
        ),
      ).rejects.toThrow();
      await expect(
        host.request({
          action: "reserve",
          fence,
          provider_attempt: randomUUID(),
          final_result: false,
          maximum: { ...maximum, tokens: 30_000_000 },
        }),
      ).rejects.toThrow();
      await writeFile(
        join(f.directory, "judgement-result.json"),
        JSON.stringify({
          assessment: firstId,
          evidence: same.evidence[0].artifact_id,
        }),
      );
    } finally {
      server.closeAllConnections();
      await new Promise<void>((resolve) => server.close(() => resolve()));
      await local.close();
    }
  },
  60000,
);
