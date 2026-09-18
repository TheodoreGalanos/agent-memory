/* Generated from Rust contracts by npm run generate. */

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

export interface TaskLocalCheck {
  allowed_providers: string[];
  completion_requirements: string[];
  definition: JudgementDefinition;
  disclosure_policy: string;
  expires_at: string;
  id: string;
  job_id: string;
  retired: boolean;
  scope: Scope;
  task_id: string;
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
export interface SourceRef {
  revision: string;
  source_id: string;
}
