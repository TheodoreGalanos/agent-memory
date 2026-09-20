import { expect, it } from "vitest";
import { requestId } from "../src/identity.js";

it("preserves the request IDs already used by formation and decision retries", () => {
  expect(requestId("job", "source", "1")).toBe("6022b070-ae56-828c-a168-998678bbdf03");
  expect(requestId("invocation", "candidates")).toBe("e7df3942-bbbf-8c0f-94a6-a83969a0c9df");
  expect(requestId("job", "source", "2")).not.toBe(requestId("job", "source", "1"));
});
