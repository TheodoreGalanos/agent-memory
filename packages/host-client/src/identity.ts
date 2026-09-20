import { createHash } from "node:crypto";

/** Stable request and operation IDs. Keep the encoding unchanged so retries find existing receipts. */
export function requestId(ownerId: string, ...parts: string[]): string {
  const digest = createHash("sha256").update([ownerId, ...parts].join(":")).digest();
  digest[6] = (digest[6] & 0x0f) | 0x80;
  digest[8] = (digest[8] & 0x3f) | 0x80;
  const hex = digest.subarray(0, 16).toString("hex");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20, 32)}`;
}
