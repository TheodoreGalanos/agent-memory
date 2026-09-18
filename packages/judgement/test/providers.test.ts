import { createServer } from "node:http";
import { once } from "node:events";
import { it, expect, vi } from "vitest";
import { Ajv } from "ajv";
import {
  createModels,
  fauxProvider,
  fauxAssistantMessage,
} from "@earendil-works/pi-ai";
import { azureOpenAIResponsesProvider } from "@earendil-works/pi-ai/providers/azure-openai-responses";
import { GenerativeProvider, JevProvider } from "../src/providers.js";
import { packet, maximum, reply } from "./fixtures.js";
import { validateResponse } from "../src/validation.js";

it("keeps malformed, empty and incomplete reference replies invalid", async () => {
  const models = createModels(),
    faux = fauxProvider(),
    p = packet();
  models.setProvider(faux.provider);
  const provider = new GenerativeProvider(
    "reference",
    models,
    faux.getModel(),
    maximum,
    2048,
  );
  const valid = JSON.stringify({ answers: reply(p).answers });
  for (const [text, stopReason] of [
    [valid, "stop"],
    ["{broken", "stop"],
    ["", "stop"],
    [valid, "length"],
  ] as const) {
    faux.setResponses([{ ...fauxAssistantMessage(text), stopReason }]);
    const result = await provider.evaluate(p, AbortSignal.timeout(2000));
    expect(result.status).toBe(200);
    if (text === valid && stopReason === "stop")
      expect(validateResponse(p, result.raw, false).answers).toEqual(
        reply(p).answers,
      );
    else expect(() => validateResponse(p, result.raw, false)).toThrow();
    if (stopReason === "length")
      expect(result.raw).toHaveProperty("incomplete_response", valid);
  }
});

it("sends a strict packet-specific schema through Pi's Azure HTTP adapter", async () => {
  const models = createModels();
  models.setProvider(azureOpenAIResponsesProvider());
  const model = models.getModel("azure-openai-responses", "gpt-4.1-mini")!;
  const p = packet();
  p.questions[0].definition.questions.push(
    {
      key: "truth",
      instructions: "Is it supported?",
      form: {
        type: "noul",
        criteria: { true: "Supported", false: "Not supported" },
      },
    },
    {
      key: "level",
      instructions: "How much support?",
      form: { type: "score", criteria: ["None", "Full"] },
    },
  );
  let body: any;
  const fetch = vi.fn(async (_input: unknown, init?: RequestInit) => {
    body = JSON.parse(init!.body as string);
    return new Response(
      JSON.stringify({
        error: { message: "Test rejection", type: "invalid_request_error" },
      }),
      { status: 400, headers: { "content-type": "application/json" } },
    );
  });
  const complete = models.completeSimple.bind(models);
  const spy = vi
    .spyOn(models, "completeSimple")
    .mockImplementation((selected, context, options) =>
      complete(selected, context, {
        ...options,
        apiKey: "test-key",
        env: {
          AZURE_OPENAI_BASE_URL: "https://example.invalid/openai/v1",
          AZURE_OPENAI_DEPLOYMENT_NAME_MAP: "",
          AZURE_OPENAI_API_VERSION: "v1",
        },
        fetch,
      }),
    );
  try {
    const provider = new GenerativeProvider(
      "reference",
      models,
      model,
      maximum,
      2048,
    );
    const result = await provider.evaluate(p, AbortSignal.timeout(10000));
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(result.status).toBe(503); // No retry or unstructured fallback on API rejection.
    expect(body.text?.format).toMatchObject({
      type: "json_schema",
      name: "judgement_answers",
      strict: true,
    });
    const validate = new Ajv().compile(body.text.format.schema);
    const answers = {
      "J01.assessment": { type: "choice", choice: "inference" },
      "J01.faithfulness": { type: "choice", choice: "yes" },
      "J01.truth": { type: "noul", noul: 0.8 },
      "J01.level": {
        type: "score",
        score: 1,
        legend: { "0": "None", "1": "Full" },
      },
    };
    expect(validate({ answers })).toBe(true);
    for (const wrong of [
      "inference",
      { type: "inference" },
      { type: "choice", choice: "invented" },
      { type: "choice", choice: "inference", confidence: 1 },
    ]) {
      expect(
        validate({ answers: { ...answers, "J01.assessment": wrong } }),
      ).toBe(false);
    }
    expect(validate({ answers: { ...answers, "J01.truth": undefined } })).toBe(
      false,
    );
    expect(validate({ answers, rationale: "extra" })).toBe(false);
    expect(
      validate({
        answers: {
          ...answers,
          "J01.level": {
            type: "score",
            score: 1,
            legend: { "0": "None", "1": "Wrong" },
          },
        },
      }),
    ).toBe(false);
  } finally {
    spy.mockRestore();
  }
});

it("enforces cancellation and retains malformed responses without inventing usage", async () => {
  const p = packet();
  let mode = "malformed";
  const server = createServer((_, res) => {
    if (mode === "wait") return;
    if (mode === "oversize") {
      res.end("x".repeat(65000));
      return;
    }
    res.end(mode === "malformed" ? "{not JSON" : JSON.stringify(reply(p)));
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const provider = new JevProvider(
    "jev",
    "test-release",
    maximum,
    () => "test-key",
    "test-release",
    `http://127.0.0.1:${(server.address() as { port: number }).port}`,
  );
  try {
    const malformed = await provider.evaluate(p, new AbortController().signal);
    expect(malformed.raw).toEqual({ invalid_json: "{not JSON" });
    expect(malformed.usage.input_tokens).toBeNull();
    mode = "wait";
    await expect(
      provider.evaluate(p, AbortSignal.timeout(40)),
    ).rejects.toThrow();
    mode = "oversize";
    await expect(
      provider.evaluate(p, AbortSignal.timeout(1000)),
    ).rejects.toThrow("retention limit");
  } finally {
    server.closeAllConnections();
    await new Promise<void>((resolve) => server.close(() => resolve()));
  }
});
it("checks the request allowance before resolving a credential", async () => {
  const credential = vi.fn(() => "test-key");
  const provider = new JevProvider(
    "jev",
    "test-release",
    { ...maximum, tokens: 1 },
    credential,
  );
  await expect(
    provider.evaluate(packet(), new AbortController().signal),
  ).rejects.toThrow("allowance");
  expect(credential).not.toHaveBeenCalled();
  expect(
    () =>
      new JevProvider(
        "jev",
        "test",
        maximum,
        credential,
        undefined,
        "http://example.com",
      ),
  ).toThrow("HTTPS");
});
