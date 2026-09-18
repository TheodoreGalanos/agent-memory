import data from "./catalogue-data.json" with { type: "json" };
import type { JudgementDefinition } from "../../../contracts/generated/judgement-packet.js";

export const catalogue = data as (JudgementDefinition & {
  behaviour: string;
})[];
export function definition(id: string): JudgementDefinition {
  const found = catalogue.find((item) => item.id === id);
  if (!found) throw new Error(`Unknown judgement family ${id}`);
  const { behaviour: _, ...contract } = found;
  return structuredClone(contract);
}
