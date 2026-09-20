// ABOUTME: Type declarations for the capability inventory script, so TS test files can import it typed.
export interface ModuleInventoryRow {
  id: string;
  version: string;
  requires: string;
  optional: string;
  operations: string;
  tools: string;
}
export interface FamilyInventoryRow {
  id: string;
  owner: string;
  permitted_uses: string;
  fixture: "yes" | "no";
}
export function inventory(): {
  markdown: string;
  modules: ModuleInventoryRow[];
  families: FamilyInventoryRow[];
};
