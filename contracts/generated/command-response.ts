/* Generated from Rust contracts by npm run generate. */

export type EffectStatus = "confirmed" | "not_performed" | "unknown";
export type ReasonCode =
  | "invalid_payload"
  | "unauthenticated"
  | "forbidden_scope"
  | "stale_revision"
  | "request_conflict"
  | "deadline_exceeded"
  | "budget_exhausted"
  | "insufficient_evidence"
  | "source_unavailable"
  | "provider_unavailable"
  | "cancelled"
  | "unknown_effect"
  | "internal_error";
export type WireVersion = "1";
export type ResponseStatus = "accepted" | "completed" | "partial" | "blocked" | "conflict" | "unavailable" | "failed";
export type UsageStatus = "known" | "partial" | "unknown";

export interface CommandResponse {
  coverage: Coverage;
  job_id?: string | null;
  known_effects: KnownEffect[];
  reason?: ContractError | null;
  request_id: string;
  result_ref?: string | null;
  schema_version: WireVersion;
  status: ResponseStatus;
  usage: Usage;
}
export interface Coverage {
  examined: string[];
  unexamined: string[];
}
export interface KnownEffect {
  description: string;
  effect_id: string;
  status: EffectStatus;
}
export interface ContractError {
  code: ReasonCode;
  message: string;
}
export interface Usage {
  cost?: Money | null;
  input_tokens?: number | null;
  output_tokens?: number | null;
  status: UsageStatus;
}
export interface Money {
  currency: string;
  minor_units: number;
}
