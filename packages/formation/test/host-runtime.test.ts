import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { randomUUID } from "node:crypto";
import { it, expect } from "vitest";
import {
  createModels,
  fauxProvider,
  fauxAssistantMessage,
} from "@earendil-works/pi-ai";
import {
  BACKGROUND_CONTEXT as context,
  value,
} from "@earendil-works/pi-agent-core";
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type {
  FormationEntry,
  FormationWindow,
} from "../../../contracts/generated/formation-window.js";
import type { JudgementPacket } from "../../../contracts/generated/judgement-packet.js";
import {
  HostClient,
  type HostCommands,
} from "../../pi-worker/src/host-client.js";
import { openLocalSession } from "../../pi-worker/src/local-session.js";
import {
  GenerativeProvider,
  type JudgementProvider,
} from "../../judgement/src/index.js";
import { fenceFor } from "../../judgement/src/packet.js";
import { maximum } from "../../judgement/test/fixtures.js";
import { runFormation } from "../src/index.js";

it.skipIf(!process.env.MEMORY_FORMATION_FIXTURE)(
  "forms bounded records through Rust, Pi and the reference provider; recovers receipts and rejects substituted evidence",
  async () => {
    const f = JSON.parse(
      await readFile(process.env.MEMORY_FORMATION_FIXTURE!, "utf8"),
    ) as {
      assignment: Assignment;
      url: string;
      token: string;
      directory: string;
      sources: FormationWindow["source"][];
    };
    const host = new HostClient(f.url, f.token),
      fence = fenceFor(f.assignment),
      brief = f.assignment.job.spec.brief;
    const local = await openLocalSession(
      join(f.directory, "formation-session"),
      f.assignment.job.session_id,
    );
    const models = createModels(),
      faux = fauxProvider();
    models.setProvider(faux.provider);
    const generative = new GenerativeProvider(
      "reference",
      models,
      faux.getModel(),
      maximum,
      2048,
      faux.getModel().id,
    );
    let calls = 0;
    const packets: JudgementPacket[] = [];
    const reference: JudgementProvider = {
      id: generative.id,
      model: generative.model,
      release: generative.release,
      maximum,
      distributions: false,
      async evaluate(packet, signal) {
        calls++;
        packets.push(packet);
        const entry = packet.evidence.find((e) => e.name === "candidate")!
          .content as unknown as FormationEntry;
        const choices: Record<string, string> = {
          "J01.assessment": entry.event.evidential_status,
          "J01.faithfulness": "yes",
          "J02.assessment":
            entry.event.event_id === "unsupported"
              ? "not_addressed"
              : "supports",
          "J03.assessment": "correction",
          "J04.assessment": "conditional_method",
          "J05.assessment": "commitment",
          "J27.assessment": "task_local",
        };
        const answers = Object.fromEntries(
          packet.questions.flatMap((d) =>
            d.definition.questions.map((q) => {
              const key = `${d.definition.id}.${q.key}`;
              return [key, { type: "choice", choice: choices[key] }];
            }),
          ),
        );
        faux.setResponses([fauxAssistantMessage(JSON.stringify({ answers }))]);
        return generative.evaluate(packet, signal);
      },
    };
    const options = {
      reference,
      disclosure: {
        policy: brief.disclosure_policy,
        scope: brief.scope,
        providers: ["reference"],
      },
    };
    const run = (
      id: string,
      source = f.sources[0],
      operation = "capture",
      limit = 32,
      client: HostCommands = host,
    ) =>
      runFormation(
        client,
        f.assignment,
        local.session,
        { id, source, operation, limit },
        options,
        context,
      );
    try {
      const beforeId = randomUUID();
      const before = await run(beforeId);
      expect(before.cursor).toBe(2);
      expect(before.records.map((r) => r.record.evidential_status)).toEqual([
        "observation",
        "inference",
      ]);
      expect(before.records[1].record.content).toMatchObject({
        family: "knowledge",
        statement: expect.stringContaining("may be absent"),
        uncertainty: expect.arrayContaining(["Not examined: type properties"]),
      });
      expect(before.coverage.unexamined).toContain("type properties");
      expect(before.inspected).toHaveLength(2);
      expect(calls).toBe(2);
      expect((await run(beforeId)).records.map((r) => r.reference)).toEqual(
        before.records.map((r) => r.reference),
      );
      expect(calls).toBe(2);
      const empty = await run(randomUUID());
      expect(empty.records).toEqual([]);
      expect(empty.cursor).toBe(2);

      const duplicate = await run(randomUUID(), f.sources[1], "capture", 2);
      expect(duplicate.records).toEqual([]);
      expect(Object.keys(duplicate.duplicate_events)).toEqual([
        "instance-empty",
        "possibly-absent",
      ]);
      expect(calls).toBe(2); // Same observation under a new source revision is not billed again.

      const afterId = randomUUID();
      let lost = false;
      const loseCommit: HostCommands = {
        async request(command, signal) {
          const response = await host.request(command, signal);
          if (command.action === "commit_formation" && !lost) {
            lost = true;
            throw new Error("lost commit response");
          }
          return response;
        },
      };
      await expect(
        run(afterId, f.sources[1], "capture", 32, loseCommit),
      ).rejects.toThrow("lost commit response");
      const billed = calls;
      const after = await run(afterId, f.sources[1]);
      expect(calls).toBe(billed);
      expect(after.cursor).toBe(11);
      expect(after.records).toHaveLength(6);
      expect(after.records.map((r) => r.record.content.family)).toEqual([
        "knowledge",
        "procedure",
        "knowledge",
        "intention",
        "episode",
        "knowledge",
      ]);
      expect(
        after.records.find((r) => r.record.label.endsWith("type-property"))
          ?.record.content,
      ).toMatchObject({ statement: expect.stringContaining("120 min") });
      const preference = after.records.find((r) =>
        r.record.label.endsWith("preference"),
      )!;
      expect(preference.record.scope).toEqual(brief.scope);
      expect(preference.record.evidential_status).toBe("attributed_statement");
      expect(
        after.records.find((r) =>
          r.record.label.endsWith("selected-hypothesis"),
        )?.record,
      ).toMatchObject({
        evidential_status: "simulation",
        availability: "historical",
        qualification: { status: "candidate" },
      });
      expect(after.deferred.map((d) => d.event_id)).toEqual([
        "scratch",
        "unselected-hypothesis",
        "unsupported",
      ]);
      expect(after.unresolved).toContainEqual(
        expect.stringContaining("unsupported"),
      );
      expect(after.coverage.unexamined).toContain(
        "Events after this bounded window or evidence cutoff",
      );
      expect(after.common_source_groups[0].event_ids).toHaveLength(9);
      expect(after.common_source_groups).toHaveLength(2);
      expect(after.records[0].record.source_locators).toHaveLength(2);
      expect(after.inspected.every((l) => l.locator.kind === "events")).toBe(
        true,
      );
      expect(after.usage.cost).toBeNull();
      expect(
        after.records.find((r) => r.record.content.family === "episode")!.record
          .derived_from,
      ).toHaveLength(3);
      expect(Object.keys(after.judgement_usage)).toHaveLength(7);
      expect(
        packets
          .find((p) => p.subject.endsWith("type-property"))
          ?.questions.map((q) => q.definition.id),
      ).toEqual(["J01", "J02", "J03"]);
      expect(
        packets
          .find((p) => p.subject.endsWith("intention"))
          ?.questions.map((q) => q.definition.id),
      ).toEqual(["J01", "J02", "J05", "J27"]);

      // Reusing a valid decision for a different event must fail at the host boundary.
      const second = await host.request({
        action: "formation_window",
        fence,
        id: randomUUID(),
        source: f.sources[0],
        operation: "substitution",
        limit: 2,
      });
      expect(second.kind).toBe("formation_window");
      if (second.kind !== "formation_window") throw new Error("missing window");
      const saved = (await local.session.getValue(
        value<{ decisions: Record<string, string> }>(
          "memory.formation",
          beforeId,
        ),
        context,
      ))!.value;
      await expect(
        host.request({
          action: "commit_formation",
          fence,
          request: { window_id: second.window.id, decisions: saved.decisions },
        }),
      ).rejects.toThrow();
      await expect(
        run(randomUUID(), f.sources[2], "rollback"),
      ).rejects.toThrow();
      await expect(
        host.request({
          action: "formation_window",
          fence,
          id: randomUUID(),
          source: { source_id: randomUUID(), revision: "private" },
          operation: "scope",
          limit: 1,
        }),
      ).rejects.toThrow();
      await expect(
        host.request({
          action: "formation_window",
          fence,
          id: randomUUID(),
          source: f.sources[0],
          operation: "limit",
          limit: 33,
        }),
      ).rejects.toThrow();
      const cutoff = await run(randomUUID(), f.sources[1]);
      expect(cutoff.cursor).toBe(11);
      expect(cutoff.records).toEqual([]);
      const beforeRepeat = calls;
      const repeat = await run(randomUUID(), f.sources[4]);
      expect(repeat.records).toEqual([]);
      expect(repeat.deferred.map((d) => d.event_id)).toContain("unsupported");
      expect(repeat.unresolved).toContainEqual(
        expect.stringContaining("unsupported"),
      );
      expect(calls).toBe(beforeRepeat);
      const independent = await run(randomUUID(), f.sources[5]);
      expect(independent.records).toHaveLength(1);
      expect(independent.records[0].record.content).toEqual(
        before.records[0].record.content,
      );
      expect(independent.records[0].reference.memory_id).not.toBe(
        before.records[0].reference.memory_id,
      );
      await writeFile(
        join(f.directory, "formation-result.json"),
        JSON.stringify({ after: afterId }),
      );
    } finally {
      await local.close();
    }
  },
  120_000,
);
