// ABOUTME: Bounded inspection of an assigned JSON artifact through the Host, selecting one value by
// ABOUTME: JSON pointer. Used by judgement packet building and scoped investigation; no executable queries.
import type { Context } from "@earendil-works/pi-agent-core";
import type { Fence } from "../../../contracts/generated/host-request.js";
import type { HostCommands } from "./client.js";

export async function inspectJson(
  host: HostCommands,
  fence: Fence,
  artifactId: string,
  pointer: string,
  context: Context,
): Promise<unknown> {
  if (pointer !== "" && !pointer.startsWith("/"))
    throw new Error("Expected a JSON pointer");
  const response = await host.request(
    {
      action: "read_input",
      fence,
      artifact_id: artifactId,
      offset: 0,
      limit: 65536,
    },
    context.abortSignal,
  );
  if (response.kind !== "artifact_data")
    throw new Error("Host did not return an assigned artifact");
  let selected: unknown = JSON.parse(response.text);
  for (const encoded of pointer ? pointer.slice(1).split("/") : []) {
    if (/~(?:[^01]|$)/.test(encoded))
      throw new Error("Invalid JSON pointer escape");
    const key = encoded.replace(/~1/g, "/").replace(/~0/g, "~");
    if (
      !selected ||
      typeof selected !== "object" ||
      !Object.hasOwn(selected, key)
    )
      throw new Error(`Required definition is missing at ${pointer}`);
    selected = (selected as Record<string, unknown>)[key];
  }
  return selected;
}
