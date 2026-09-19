// ABOUTME: Scope predicate shared by Pi workspace access checks and judgement disclosure rules:
// ABOUTME: a grant permits a request only when every dimension the grant constrains is satisfied.
import type { Scope } from "../../../contracts/generated/workspace-state.js";

export function permits(grant: Scope, requested: Scope): boolean {
  return (
    (["user_id", "project_id", "task_id"] as const).every(
      (key) => !grant[key] || grant[key] === requested[key],
    ) &&
    (!grant.entity_ids.length ||
      (requested.entity_ids.length > 0 &&
        requested.entity_ids.every((id) => grant.entity_ids.includes(id)))) &&
    (!grant.source_versions.length ||
      (requested.source_versions.length > 0 &&
        requested.source_versions.every((ref) =>
          grant.source_versions.some(
            (g) => g.source_id === ref.source_id && g.revision === ref.revision,
          ),
        )))
  );
}
