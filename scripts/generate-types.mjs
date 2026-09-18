import { readFile, writeFile } from "node:fs/promises";
import { compile } from "json-schema-to-typescript";

const directory = new URL("../contracts/generated/", import.meta.url);
for (const name of [
  "maintenance-review",
  "maintenance-result",
  "intention-check",
  "intention-occurrence",
  "consolidation-review",
  "consolidation-result",
  "qualification-input",
  "qualification-report",
  "qualification-suite",
  "adoption-result",
  "activation-window",
  "context-package",
  "formation-window",
  "formation-result",
  "judgement-packet",
  "semantic-assessment",
  "judgement-decision",
  "task-local-check",
  "work-command",
  "work-result",
  "command-response",
  "host-request",
  "host-response",
  "assignment",
  "workspace-state",
  "investigation-plan",
  "render-manifest",
]) {
  const schema = JSON.parse(
    await readFile(new URL(`${name}.schema.json`, directory)),
  );
  const types = await compile(schema, schema.title, {
    bannerComment: "/* Generated from Rust contracts by npm run generate. */",
    additionalProperties: false,
  });
  await writeFile(new URL(`${name}.ts`, directory), types);
}
