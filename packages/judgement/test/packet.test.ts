import { it, expect, vi } from "vitest";
import { BACKGROUND_CONTEXT as context } from "@earendil-works/pi-agent-core";
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type { HostCommands } from "../../pi-worker/src/host-client.js";
import { input } from "../../pi-worker/test/fixtures.js";
import { packet } from "./fixtures.js";
import { buildPacket, type PacketInput } from "../src/packet.js";
import { definition } from "../src/catalogue.js";

function setup() {
  const p = packet();
  const brief = input("before-correction").command.payload;
  const assignment = {
    job: { id: p.job_id, deadline: p.deadline, spec: { brief } },
    owner_id: "owner",
    epoch: 1,
  } as Assignment;
  const request = vi.fn<HostCommands["request"]>(async (command) => {
    if (command.action === "read_input")
      return {
        kind: "artifact_data",
        text: JSON.stringify({
          candidate: "observed door",
          evidence: "site inspection",
          claim: "door exists",
        }),
      };
    if (command.action === "admit_judgement")
      return { kind: "judgement_packet", packet: command.packet };
    throw new Error("Unexpected command");
  });
  const options: PacketInput = {
    id: p.id,
    subject: p.subject,
    frame: p.frame,
    fields: p.evidence.map(({ content, ...e }) => e),
    questions: p.questions,
    allowedProviders: p.allowed_providers,
    freshAfter: p.fresh_after,
    expiresAt: p.expires_at,
  };
  return { assignment, host: { request }, options };
}
it("batches independent selected families over the same inspected evidence", async () => {
  const { assignment, host, options } = setup();
  options.fields.push({
    ...options.fields[0],
    name: "claim",
    pointer: "/claim",
  });
  options.questions.push({
    ...options.questions[0],
    definition: definition("J02"),
  });
  const p = await buildPacket(host, assignment, options, context);
  expect(p.questions.map((q) => q.definition.id)).toEqual(["J01", "J02"]);
  expect(p.evidence.find((e) => e.name === "claim")?.content).toBe(
    "door exists",
  );
  expect(p.missing).toEqual([]);
});
it.each(["dependency", "disclosure"])(
  "rejects %s incompatibility before reading or disclosing evidence",
  async (fault) => {
    const { assignment, host, options } = setup();
    if (fault === "dependency") options.questions[0].depends_on = ["J01"];
    else options.questions[0].allowed_providers = ["reference"];
    await expect(
      buildPacket(host, assignment, options, context),
    ).rejects.toThrow();
    expect(host.request).not.toHaveBeenCalled();
  },
);
it("records missing material independently from a service failure", async () => {
  const { assignment, host, options } = setup();
  options.fields = options.fields.filter((e) => e.name !== "candidate");
  expect(
    (await buildPacket(host, assignment, options, context)).missing,
  ).toEqual(["candidate"]);
});
