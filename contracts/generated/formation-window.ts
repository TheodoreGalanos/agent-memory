/* Generated from Rust contracts by npm run generate. */

export type EvidentialStatus = "observation" | "attributed_statement" | "inference" | "assumption" | "simulation";
export type Origin = "observed" | "activated" | "agent_generated";
export type MemoryContent =
  | {
      actions: string[];
      corrections: string[];
      family: "episode";
      initial_conditions: string[];
      objective: string;
      observations: string[];
      outcome: string;
      uncertainty: string[];
      verification: string[];
    }
  | {
      examined_coverage: string[];
      family: "knowledge";
      predicate?: string | null;
      statement: string;
      subject?: string | null;
      uncertainty: string[];
    }
  | {
      applicability: string[];
      capabilities: string[];
      contract?: ProcedureContract | null;
      counterexamples: MemoryRef[];
      exclusions: string[];
      family: "procedure";
      method: ProcedureForm;
      purpose: string;
    }
  | {
      completion: string[];
      expires_at?: string | null;
      family: "intention";
      notification_policy: string;
      owner_id: string;
      plan?: IntentionPlan | null;
      purpose: string;
      readiness: string[];
      recurrence?: string | null;
      trigger: string;
    };
export type QuestionForm =
  | {
      criteria: {
        [k: string]: string;
      };
      type: "choice";
    }
  | {
      criteria: {
        [k: string]: string;
      };
      type: "noul";
    }
  | {
      criteria: string[];
      type: "score";
    };
export type ReplayClass = "observation" | "remote_idempotent" | "reconcile";
export type ProcedureForm =
  | {
      evidence_criteria: string[];
      form: "advisory";
      steps: string[];
    }
  | {
      artifact_id: string;
      entrypoint: string;
      form: "executable";
      inputs: string[];
      outputs: string[];
    };
export type Process = "formation" | "activation" | "consolidation" | "maintenance" | "investigation" | "evaluation";
export type WireVersion = "1";
export type IntentionTrigger =
  | {
      at: string;
      kind: "time";
    }
  | {
      event_kind: string;
      kind: "event";
      resource_id: string;
    }
  | {
      after_revision: string;
      kind: "source_revision";
      source_id: string;
    }
  | {
      job_id: string;
      kind: "result";
    }
  | {
      definition: JudgementDefinition;
      kind: "semantic";
      matched_choice: string;
    };
export type Retention = "required" | "optional" | "temporary";
export type Locator =
  | {
      end_line: number;
      kind: "document";
      page?: number | null;
      start_line: number;
    }
  | {
      column?: string | null;
      kind: "table";
      row: number;
    }
  | {
      entity: string;
      kind: "model";
      property: string;
    }
  | {
      kind: "image";
      /**
       * @minItems 4
       * @maxItems 4
       */
      region: [number, number, number, number];
    }
  | {
      end: number;
      kind: "events";
      start: number;
    };

export interface FormationWindow {
  after: number;
  cutoff: string;
  entries: FormationEntry[];
  id: string;
  job_id: string;
  operation: string;
  policy: ConfigRef;
  remaining: boolean;
  scope: Scope;
  source: SourceRef;
  through: number;
}
export interface FormationEntry {
  duplicate_records?: MemoryRef[] | null;
  event: ToolEvent;
  input: FormationInput;
  locator: SourceLocator;
  native_locators: SourceLocator[];
  position: number;
  previous_deferral?: DeferredContribution | null;
}
export interface MemoryRef {
  label: string;
  memory_id: string;
  revision: number;
}
export interface ToolEvent {
  content: unknown;
  event_id: string;
  evidential_status: EvidentialStatus;
  kind: string;
  observed_at: string;
  origin: Origin;
}
/**
 * Supplied by the source connector or explicit contribution UI, not inferred
 * from text claiming to be a user or an observation.
 */
export interface FormationInput {
  actor_id?: string | null;
  based_on?: string[];
  boundary_explicit: boolean;
  conditions: string[];
  content: MemoryContent;
  corrects: string[];
  coverage: Coverage;
  episode: string;
  evidence: unknown;
  explicit_contribution: boolean;
  explicitly_selected: boolean;
  objective: string;
  retention: Retention;
  source_locators: string[];
  uncertainty: string[];
}
export interface ProcedureContract {
  checks: string[];
  decision_points: string[];
  effects: string[];
  parameters: {
    [k: string]: string;
  };
  recognition: RecognitionCriterion[];
  replay_class: ReplayClass;
  stopping_criteria: string[];
}
export interface RecognitionCriterion {
  definition: JudgementDefinition;
  model: string;
  model_release: string;
  policy: ConfigRef;
}
export interface JudgementDefinition {
  applicability: string;
  evaluation_refs: string[];
  fallback: string;
  id: string;
  input_requirements: string[];
  owner: string;
  permitted_uses: string[];
  questions: JudgementQuestion[];
  revision: number;
}
export interface JudgementQuestion {
  form: QuestionForm;
  instructions: string;
  key: string;
}
export interface ConfigRef {
  id: string;
  label: string;
  revision: number;
}
export interface IntentionPlan {
  completion: CompletionRule;
  execution: WorkBrief;
  ready_artifacts: string[];
  ready_memories: MemoryRef[];
  /**
   * Fixed intervals only; missed windows expire without a backlog of executions.
   */
  recurrence_seconds?: number | null;
  trigger: IntentionTrigger;
}
export interface CompletionRule {
  confirmation_owner?: string | null;
  /**
   * When set, a host-recorded job completion before expiry can be delivered within this grace.
   */
  delivery_grace_seconds?: number | null;
  equals: unknown;
  /**
   * Invocation IDs within the execution job. The Host supplies the operation prefix.
   */
  required_effects: string[];
  result_pointer: string;
  semantic_conditions: string[];
}
export interface WorkBrief {
  capabilities: Capabilities;
  definitions: string[];
  disclosure_policy: string;
  evidence_cutoff: string;
  frame_ref?: string | null;
  freshness_requirement: string;
  inputs: WorkInputs;
  known_conflicts: string[];
  limits: WorkLimits;
  output_criteria: string[];
  policy: ConfigRef;
  process: Process;
  profile: ConfigRef;
  purpose: string;
  retention_policy: string;
  schema_version: WireVersion;
  scope: Scope;
  task_id: string;
}
export interface Capabilities {
  queries: string[];
  sources: SourceRef[];
  tools: string[];
}
export interface SourceRef {
  revision: string;
  source_id: string;
}
export interface WorkInputs {
  artifacts: string[];
  memories: MemoryRef[];
  sources: SourceRef[];
}
export interface WorkLimits {
  max_child_concurrency: number;
  max_child_depth: number;
  max_output_bytes: number;
  max_provider_attempts: number;
  max_tokens: number;
  root_budget_id: string;
}
/**
 * Absent dimensions are unrestricted. An assigned restriction cannot be removed
 * by leaving that dimension out of a request. Tenant is checked separately.
 */
export interface Scope {
  entity_ids: string[];
  project_id?: string | null;
  source_versions: SourceRef[];
  task_id?: string | null;
  user_id?: string | null;
}
export interface Coverage {
  examined: string[];
  unexamined: string[];
}
export interface SourceLocator {
  id: string;
  locator: Locator;
  source: SourceRef;
}
export interface DeferredContribution {
  event_id: string;
  reason: string;
  required: boolean;
}
