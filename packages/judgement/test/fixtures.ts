import { randomUUID } from "node:crypto";
import { input } from "../../pi-worker/test/fixtures.js";
import type { JudgementPacket } from "../../../contracts/generated/judgement-packet.js";
import type { SemanticAssessment } from "../../../contracts/generated/semantic-assessment.js";
import { definition } from "../src/catalogue.js";
import fixtures from "../../../evals/judgement/fixtures.json" with { type: "json" };
export { fixtures };
export const maximum = {
  tokens: 100000,
  provider_calls: 1,
  cost_microunits: 0,
  sandbox_time_ms: 0,
  sandbox_cpu_ms: 0,
  output_bytes: 0,
};
export function packet(family = "J01"): JudgementPacket {
  const brief = input("before-correction").command.payload;
  const fixture = fixtures.find((f) => f.family === family)!;
  const artifact = randomUUID();
  return {
    id: randomUUID(),
    job_id: randomUUID(),
    subject: fixture.subject,
    frame: "Assess the supplied example only",
    scope: brief.scope,
    policy: brief.policy,
    disclosure_policy: brief.disclosure_policy,
    budget_id: brief.limits.root_budget_id,
    evidence_cutoff: new Date().toISOString(),
    fresh_after: new Date(Date.now() - 60000).toISOString(),
    deadline: new Date(Date.now() + 60000).toISOString(),
    expires_at: new Date(Date.now() + 120000).toISOString(),
    inputs: { sources: [], memories: [], artifacts: [artifact] },
    evidence: Object.entries(fixture.fields).map(([name, content]) => ({
      name,
      content,
      artifact_id: artifact,
      pointer: `/${name}`,
      origin: "observed",
      coverage: [name],
    })),
    missing: [],
    allowed_providers: ["jev", "reference"],
    questions: [
      {
        definition: definition(family),
        disclosure_scope: brief.scope,
        allowed_providers: ["jev", "reference"],
        depends_on: [],
      },
    ],
  };
}
export function reply(p: JudgementPacket, insufficient = false) {
  return {
    model: "test-release",
    answers: Object.fromEntries(
      p.questions.flatMap(({ definition: d }) =>
        d.questions.map((q) => {
          const selected = insufficient
            ? "insufficient_evidence"
            : ((
                fixtures.find((f) => f.family === d.id)?.answers as
                  Record<string, string> | undefined
              )?.[q.key] ?? "yes");
          if (q.form.type !== "choice")
            throw new Error("Use explicit primitive fixture");
          return [
            `${d.id}.${q.key}`,
            {
              type: "choice",
              choice: selected,
              probabilities: Object.fromEntries(
                Object.keys(q.form.criteria).map((c) => [
                  c,
                  c === selected ? 1 : 0,
                ]),
              ),
              confidence: 1,
            },
          ];
        }),
      ),
    ),
    usage: { input_tokens: 20, output_tokens: 5 },
  };
}
export function assessment(p: JudgementPacket): SemanticAssessment {
  return {
    id: randomUUID(),
    packet_id: p.id,
    raw_artifact_id: randomUUID(),
    reservation_id: randomUUID(),
    provider: "jev",
    requested_model: "test-release",
    returned_model: "test-release",
    model_release: "test-release",
    status: "answered",
    answers: reply(p).answers as SemanticAssessment["answers"],
    usage: { input_tokens: 20, output_tokens: 5 },
    assessed_at: new Date().toISOString(),
    expires_at: p.expires_at,
  };
}
