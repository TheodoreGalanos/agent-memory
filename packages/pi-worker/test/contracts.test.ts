import { spawnSync } from "node:child_process";
import { Ajv2020 } from "ajv/dist/2020.js";
import { fullFormats } from "ajv-formats/dist/formats.js";
import { describe, expect, it } from "vitest";
import { checkpoints, input, loadJson, scriptedResult } from "./fixtures.js";
import world from "../../../evals/property-location/reference-world.json" with { type: "json" };

const ajv = new Ajv2020({ allErrors: true });
ajv.addFormat("uuid", fullFormats.uuid);
ajv.addFormat("date-time", fullFormats["date-time"]);
const command = ajv.compile(
  loadJson("contracts/generated/work-command.schema.json") as object,
);
const result = ajv.compile(
  loadJson("contracts/generated/work-result.schema.json") as object,
);
const asp = ajv.compile(
  loadJson("contracts/upstream/asp.schema.json") as object,
);

function rust(kind: string, value: unknown) {
  return spawnSync(
    "cargo",
    ["run", "--quiet", "--example", "validate_contract", "--", kind],
    {
      cwd: new URL("../../../", import.meta.url),
      input: JSON.stringify(value),
      encoding: "utf8",
    },
  );
}

describe("generated wire contracts (T01)", () => {
  it.each(checkpoints)("accepts %s in TypeScript and Rust", (name) => {
    for (const [kind, value, validate] of [
      ["work-command", input(name).command, command],
      ["work-result", scriptedResult(name), result],
    ] as const) {
      expect(validate(value), JSON.stringify(validate.errors)).toBe(true);
      const checked = rust(kind, value);
      expect(checked.status, checked.stderr).toBe(0);
      // Check the Rust representation still satisfies the same wire schema.
      expect(validate(JSON.parse(checked.stdout))).toBe(true);
    }
  });

  it.each([0, -1, 1.5, 4_294_967_296])(
    "rejects invalid revision %s in both languages",
    (revision) => {
      const value = input("before-correction").command;
      value.payload.policy.revision = revision;
      expect(command(value)).toBe(false);
      expect(rust("work-command", value).status).not.toBe(0);
    },
  );

  it("rejects unknown statuses, malformed IDs and invalid dates", () => {
    expect(
      result({
        ...scriptedResult("after-correction"),
        status: "probably_complete",
      }),
    ).toBe(false);
    expect(
      command({
        ...input("before-correction").command,
        request_id: "not-an-id",
      }),
    ).toBe(false);
    expect(
      command({ ...input("before-correction").command, deadline: "tomorrow" }),
    ).toBe(false);
  });
});

describe("reference-world separation (T15 fixture infrastructure)", () => {
  it.each(checkpoints)(
    "%s exposes only evidence available at its cutoff",
    (name) => {
      const { command, evidence } = input(name);
      for (const observation of evidence) {
        expect(Date.parse(observation.observed_at)).toBeLessThanOrEqual(
          Date.parse(command.payload.evidence_cutoff),
        );
        expect(command.payload.inputs.sources).toContainEqual(
          observation.source,
        );
      }
      expect(input(name)).not.toHaveProperty("facts");
      expect(input(name)).not.toHaveProperty("checkpoints");
      expect(evidence.map((observation) => observation.id)).toEqual(
        world.checkpoints[name].visible_evidence,
      );
      expect(scriptedResult(name).status).toBe(world.checkpoints[name].status);
      expect(JSON.stringify(input(name))).not.toContain(
        world.facts.D.instance_values.FireResistance,
      );
    },
  );

  it("preserves the correction sequence without revealing D's answer", () => {
    expect(input("before-correction").evidence.map((x) => x.id)).toEqual([
      "instance-empty",
    ]);
    expect(input("after-correction").evidence.map((x) => x.id)).toContain(
      "type-property",
    );
    expect(
      input("after-correction").evidence.find((x) => x.id === "type-property")
        ?.rows,
    ).toContainEqual({
      type_id: world.facts.C.type,
      FireResistance: world.facts.C.type_values.FireResistance,
    });
    expect(
      scriptedResult("before-correction").findings.some(
        (f) => f.evidential_status === "inference",
      ),
    ).toBe(true);
    expect(
      scriptedResult("after-correction").findings.at(-1)?.applicability
        .source_versions[0].revision,
    ).toBe("C");
    expect(scriptedResult("revision-d").status).toBe("blocked");
    expect(input("revision-d").evidence).toHaveLength(0);
  });
});

describe("pinned ASP v0 schema", () => {
  const descriptor = {
    version: "0",
    transport: "ssh",
    connection: { host: "127.0.0.1", port: 2222 },
    workspace: "/workspace",
  };
  it("accepts an SSH descriptor", () => expect(asp(descriptor)).toBe(true));
  it("rejects unknown versions, transports and ambiguous key sources", () => {
    expect(asp({ ...descriptor, version: "1" })).toBe(false);
    expect(asp({ ...descriptor, transport: "local" })).toBe(false);
    expect(
      asp({
        ...descriptor,
        connection: { host: "host", identity: { env: "KEY", file: "/key" } },
      }),
    ).toBe(false);
    expect(asp({ ...descriptor, workspace: "relative" })).toBe(false);
  });
});
