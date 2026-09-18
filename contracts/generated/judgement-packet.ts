/* Generated from Rust contracts by npm run generate. */

export type Origin = "observed" | "activated" | "agent_generated";
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

export interface JudgementPacket {
  allowed_providers: string[];
  budget_id: string;
  deadline: string;
  disclosure_policy: string;
  evidence: JudgementEvidence[];
  evidence_cutoff: string;
  expires_at: string;
  frame: string;
  fresh_after: string;
  id: string;
  inputs: WorkInputs;
  job_id: string;
  local_check_id?: string | null;
  missing: string[];
  policy: ConfigRef;
  questions: PacketQuestion[];
  scope: Scope;
  subject: string;
}
export interface JudgementEvidence {
  artifact_id: string;
  content: unknown;
  coverage: string[];
  name: string;
  origin: Origin;
  pointer: string;
}
export interface WorkInputs {
  artifacts: string[];
  memories: MemoryRef[];
  sources: SourceRef[];
}
export interface MemoryRef {
  label: string;
  memory_id: string;
  revision: number;
}
export interface SourceRef {
  revision: string;
  source_id: string;
}
export interface ConfigRef {
  id: string;
  label: string;
  revision: number;
}
export interface PacketQuestion {
  allowed_providers: string[];
  definition: JudgementDefinition;
  /**
   * Named evidence fields containing results from earlier packets.
   */
  depends_on: string[];
  disclosure_scope: Scope;
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
