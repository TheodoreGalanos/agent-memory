/* Generated from Rust contracts by npm run generate. */

export interface InvestigationPlan {
  definitions: DefinitionLookup[];
  interpretation_conditions: string[];
  method: ConfigRef;
  steps: InvestigationStep[];
}
export interface DefinitionLookup {
  artifact_id: string;
  name: string;
  /**
   * RFC 6901 pointer into an assigned JSON artifact.
   */
  pointer: string;
}
export interface ConfigRef {
  id: string;
  label: string;
  revision: number;
}
export interface InvestigationStep {
  inputs: WorkInputs;
  key: string;
  output_criteria: string[];
  question: string;
  reuse_job_id?: string | null;
  tools: string[];
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
