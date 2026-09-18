/* Generated from Rust contracts by npm run generate. */

export interface QualificationSuite {
  cases: QualificationCase[];
  evaluator_profile: ConfigRef;
}
export interface QualificationCase {
  conditions: string[];
  expected: unknown;
  id: string;
  /**
   * All variants/restatements of a source must share this group.
   */
  source_group: string;
  task: unknown;
}
export interface ConfigRef {
  id: string;
  label: string;
  revision: number;
}
