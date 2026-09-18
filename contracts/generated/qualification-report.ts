/* Generated from Rust contracts by npm run generate. */

export type QualificationArm = "episodes" | "summary" | "procedure";

export interface QualificationReport {
  candidate: MemoryRef;
  evaluator_profile: ConfigRef;
  review_id: string;
  suite_id: string;
  trials: QualificationTrial[];
  unresolved: string[];
}
export interface MemoryRef {
  label: string;
  memory_id: string;
  revision: number;
}
export interface ConfigRef {
  id: string;
  label: string;
  revision: number;
}
export interface QualificationTrial {
  answer: unknown;
  arm: QualificationArm;
  case_id: string;
  cost_microunits: number;
  evidence_artifact: string;
  recognition: RecognitionObservation[];
}
export interface RecognitionObservation {
  answer_correct: boolean;
  criterion: string;
  false_negative: boolean;
  packet_valid: boolean;
  routing_correct: boolean;
}
