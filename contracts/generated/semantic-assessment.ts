/* Generated from Rust contracts by npm run generate. */

export type JudgementAnswer =
  | {
      choice: string;
      confidence?: number | null;
      probabilities?: {
        [k: string]: number;
      } | null;
      type: "choice";
    }
  | {
      noul: number;
      type: "noul";
    }
  | {
      confidence?: number | null;
      legend: {
        [k: string]: string;
      };
      probabilities?: {
        [k: string]: number;
      } | null;
      score: number;
      type: "score";
    };
export type AssessmentStatus = "answered" | "invalid_response" | "unavailable";

export interface SemanticAssessment {
  answers: {
    [k: string]: JudgementAnswer;
  };
  assessed_at: string;
  expires_at: string;
  failure?: string | null;
  id: string;
  model_release?: string | null;
  packet_id: string;
  provider: string;
  raw_artifact_id: string;
  requested_model: string;
  reservation_id: string;
  returned_model?: string | null;
  status: AssessmentStatus;
  usage: JudgementUsage;
}
export interface JudgementUsage {
  cost_microunits?: number | null;
  input_tokens?: number | null;
  output_tokens?: number | null;
}
