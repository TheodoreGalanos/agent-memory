import type { HostRequest } from "../../../contracts/generated/host-request.js";
import type { HostResponse } from "../../../contracts/generated/host-response.js";

export interface HostCommands {
  request(command: HostRequest, signal?: AbortSignal): Promise<HostResponse>;
}

export class HostClient implements HostCommands {
  private readonly endpoint: URL;
  private readonly token: string;
  constructor(baseUrl: string, token: string) {
    this.token = token;
    this.endpoint = new URL("/v1/commands", baseUrl);
    if (
      this.endpoint.protocol !== "https:" &&
      !(
        this.endpoint.protocol === "http:" &&
        ["127.0.0.1", "[::1]", "localhost"].includes(this.endpoint.hostname)
      )
    ) {
      throw new Error("The host connection requires HTTPS or local loopback");
    }
    if (this.endpoint.username || this.endpoint.password || token.length < 32)
      throw new Error("Invalid host connection configuration");
  }
  async request(
    command: HostRequest,
    signal?: AbortSignal,
  ): Promise<HostResponse> {
    const response = await fetch(this.endpoint, {
      method: "POST",
      redirect: "error",
      headers: {
        "content-type": "application/json",
        authorization: `Bearer ${this.token}`,
      },
      body: JSON.stringify(command),
      signal: AbortSignal.any([
        AbortSignal.timeout(30_000),
        ...(signal ? [signal] : []),
      ]),
    });
    if (!response.body) throw new Error("Host returned no response body");
    const reader = response.body.getReader();
    const chunks: Uint8Array[] = [];
    let length = 0;
    try {
      while (true) {
        const chunk = await reader.read();
        if (chunk.done) break;
        length += chunk.value.length;
        if (length > 2 * 1024 * 1024)
          throw new Error("Host response exceeds 2 MiB");
        chunks.push(chunk.value);
      }
    } finally {
      await reader.cancel();
    }
    const body = Buffer.concat(chunks).toString("utf8");
    // Error bodies may be plain text from the transport layer, not command JSON.
    if (!response.ok)
      throw new Error(`Host command failed (${response.status}): ${body.slice(0, 500)}`);
    const result: unknown = JSON.parse(body);
    if (!result || typeof result !== "object" || !("kind" in result))
      throw new Error("Host returned an invalid command response");
    return result as HostResponse;
  }
}
