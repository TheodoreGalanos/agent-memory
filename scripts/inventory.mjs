// ABOUTME: Prints a Markdown inventory of every pool module (POOL_MODULES) and judgement catalogue family,
// ABOUTME: including offered tools and whether each family has an eval fixture, for `npm run inventory`.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { POOL_MODULES } from "../packages/supervisor/src/index.js";
import { toolName } from "../packages/pi-worker/src/harness.js";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function moduleTable() {
  const rows = POOL_MODULES.map((module) => {
    const { id, version, requires, optional, operations } = module.declaration;
    const tools = (module.tools?.({}) ?? []).map(toolName);
    return {
      id,
      version,
      requires: requires.length ? requires.join(", ") : "—",
      optional: optional?.length ? optional.join(", ") : "—",
      operations: operations.length ? operations.join(", ") : "—",
      tools: tools.length ? tools.join(", ") : "—",
    };
  });
  const header = ["id", "version", "requires", "optional", "operations", "tools offered"];
  const lines = [
    `| ${header.join(" | ")} |`,
    `| ${header.map(() => "---").join(" | ")} |`,
    ...rows.map((r) => `| ${r.id} | ${r.version} | ${r.requires} | ${r.optional} | ${r.operations} | ${r.tools} |`),
  ];
  return { markdown: lines.join("\n"), rows };
}

function familyTable() {
  const catalogue = JSON.parse(readFileSync(join(root, "packages/judgement/src/catalogue-data.json"), "utf8"));
  const fixtures = JSON.parse(readFileSync(join(root, "evals/judgement/fixtures.json"), "utf8"));
  const fixtureFamilies = new Set(fixtures.map((f) => f.family));
  const rows = catalogue.map((family) => ({
    id: family.id,
    owner: family.owner,
    permitted_uses: family.permitted_uses?.length ? family.permitted_uses.join(", ") : "—",
    fixture: fixtureFamilies.has(family.id) ? "yes" : "no",
  }));
  const header = ["id", "owner", "permitted uses", "eval fixture"];
  const lines = [
    `| ${header.join(" | ")} |`,
    `| ${header.map(() => "---").join(" | ")} |`,
    ...rows.map((r) => `| ${r.id} | ${r.owner} | ${r.permitted_uses} | ${r.fixture} |`),
  ];
  return { markdown: lines.join("\n"), rows };
}

export function inventory() {
  const modules = moduleTable();
  const families = familyTable();
  const markdown = [
    "# Capability inventory",
    "",
    `## Pool modules (${modules.rows.length})`,
    "",
    modules.markdown,
    "",
    `## Judgement catalogue families (${families.rows.length})`,
    "",
    families.markdown,
    "",
  ].join("\n");
  return { markdown, modules: modules.rows, families: families.rows };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  console.log(inventory().markdown);
}
