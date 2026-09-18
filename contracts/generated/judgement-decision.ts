/* Generated from Rust contracts by npm run generate. */

export type PolicyAction =
  "retain" | "qualify" | "retrieve_further" | "investigate" | "revise" | "defer" | "retire" | "request_input";

export interface JudgementDecision {
  assessment_ids: string[];
  decisions: {
    [k: string]: PolicyDecision;
  };
  id: string;
  inconsistencies: string[];
  mode: string;
  packet_id: string;
  selected_assessment?: string | null;
}
export interface PolicyDecision {
  action: PolicyAction;
  constraints: string[];
  expires_at?: string | null;
  policy: ConfigRef;
  reason: string;
  required_evidence: string[];
}
export interface ConfigRef {
  id: string;
  label: string;
  revision: number;
}
