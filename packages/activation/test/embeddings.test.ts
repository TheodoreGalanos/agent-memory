import { expect, it, vi, beforeEach } from "vitest";
import type { HostCommands } from "../../pi-worker/src/host-client.js";
import type { HostRequest } from "../../../contracts/generated/host-request.js";
import {
  NomicEmbeddings,
  EmbeddingInputTooLong,
  indexEmbeddingPage,
} from "../src/embeddings.js";

const fake = vi.hoisted(() => ({
  tokens: 10,
  data: [1, ...Array(767).fill(0)],
  extract: vi.fn(),
  tokenize: vi.fn(),
  dispose: vi.fn(),
  load: vi.fn(),
}));
vi.mock("@huggingface/transformers", () => ({
  AutoTokenizer: { from_pretrained: async () => fake.tokenize },
  AutoModel: { from_pretrained: fake.load },
  FeatureExtractionPipeline: class {
    constructor() {
      return Object.assign(fake.extract, {
        tokenizer: fake.tokenize,
        dispose: fake.dispose,
      });
    }
  },
  layer_norm: () => ({ normalize: () => ({ data: fake.data }) }),
}));
beforeEach(() => {
  vi.clearAllMocks();
  fake.tokens = 10;
  fake.data = [1, ...Array(767).fill(0)];
  fake.extract.mockResolvedValue({ dims: [1, 768] });
  fake.tokenize.mockImplementation(() => ({
    input_ids: { dims: [1, fake.tokens] },
  }));
  fake.load.mockResolvedValue(
    Object.assign(fake.extract, {
      tokenizer: fake.tokenize,
      dispose: fake.dispose,
    }),
  );
});

it("uses the q8 CPU model, retrieval prefixes and one compatible identity", async () => {
  const model = await NomicEmbeddings.load({ cacheDir: "/test/cache" });
  expect(fake.load).toHaveBeenCalledWith(
    expect.stringContaining("/test/cache/nomic-ai/nomic-embed-text-v1.5/"),
    expect.objectContaining({
      dtype: "q8",
      device: "cpu",
      local_files_only: true,
      cache_dir: "/test/cache",
    }),
  );
  const doc = await model.document("Valve is closed");
  const query = await model.query("Is the valve shut?");
  expect(fake.tokenize).toHaveBeenCalledWith(
    "search_document: Valve is closed",
    { truncation: false, padding: false },
  );
  expect(fake.extract).toHaveBeenCalledWith(
    "search_query: Is the valve shut?",
    { pooling: "mean" },
  );
  expect(query.identity).toEqual(doc.identity);
  expect(doc.values).toHaveLength(768);
  await model.dispose();
  expect(fake.dispose).toHaveBeenCalledOnce();
});

it("rejects oversize and empty input before inference, and invalid model output", async () => {
  const model = await NomicEmbeddings.load({
    cacheDir: "/test/cache",
    allowDownload: true,
  });
  expect(fake.load.mock.calls[0][1].local_files_only).toBe(false);
  await expect(model.query(" ")).rejects.toThrow("empty");
  fake.tokens = 8193;
  await expect(model.document("large record")).rejects.toThrow(
    EmbeddingInputTooLong,
  );
  expect(fake.extract).not.toHaveBeenCalled();
  fake.tokens = 8192;
  await model.document("fits");
  fake.data[0] = NaN;
  await expect(model.query("test")).rejects.toThrow("invalid embedding");
});

it("does not publish an embedding when cancelled during inference", async () => {
  const model = await NomicEmbeddings.load({ cacheDir: "/test/cache" });
  const controller = new AbortController();
  fake.extract.mockImplementation(async () => {
    controller.abort();
    return { dims: [1, 768] };
  });
  await expect(model.query("test", controller.signal)).rejects.toThrow();
});

const inputs = [1, 2, 3].map((n) => ({
  memory: { memory_id: `memory-${n}`, revision: 1, label: `Memory ${n}` },
  text: `Record ${n}`,
  cursor: { sequence: n, version_id: `version-${n}` },
}));

it("indexes a bounded page, reports oversized records and returns the resume cursor", async () => {
  const requests: HostRequest[] = [];
  const host: HostCommands = {
    async request(command) {
      requests.push(command);
      return command.action === "embedding_inputs"
        ? { kind: "embedding_inputs", inputs }
        : { kind: "done", affected: 1 };
    },
  };
  const model = await NomicEmbeddings.load({ cacheDir: "/test/cache" });
  const document = vi.spyOn(model, "document");
  document.mockRejectedValueOnce(new EmbeddingInputTooLong(9000));
  const after = { sequence: 0, version_id: "previous" };
  const result = await indexEmbeddingPage(host, model, { after, limit: 3 });
  expect(requests[0]).toEqual({ action: "embedding_inputs", after, limit: 3 });
  expect(result).toMatchObject({
    indexed: 2,
    after: inputs[2].cursor,
    exhausted: false,
    skipped: [{ memory: inputs[0].memory }],
  });
  expect(requests.filter((r) => r.action === "save_embedding")).toHaveLength(2);
});

it("stops on a failed save rather than advancing past unsaved work", async () => {
  const request = vi
    .fn<HostCommands["request"]>()
    .mockResolvedValueOnce({ kind: "embedding_inputs", inputs })
    .mockRejectedValue(new Error("Host rejected stale revision"));
  const model = await NomicEmbeddings.load({ cacheDir: "/test/cache" });
  await expect(indexEmbeddingPage({ request }, model)).rejects.toThrow(
    "stale revision",
  );
  expect(request).toHaveBeenCalledTimes(2);
});

it("keeps the cursor on an empty page and validates bounds before calling the host", async () => {
  const request = vi
    .fn<HostCommands["request"]>()
    .mockResolvedValue({ kind: "embedding_inputs", inputs: [] });
  const model = await NomicEmbeddings.load({ cacheDir: "/test/cache" });
  await expect(
    indexEmbeddingPage({ request }, model, { limit: 101 }),
  ).rejects.toThrow("limit");
  await expect(
    indexEmbeddingPage({ request }, model, { signal: AbortSignal.abort() }),
  ).rejects.toThrow();
  expect(request).not.toHaveBeenCalled();
  const after = inputs[2].cursor;
  expect(await indexEmbeddingPage({ request }, model, { after })).toEqual({
    indexed: 0,
    skipped: [],
    after,
    exhausted: true,
  });
});
