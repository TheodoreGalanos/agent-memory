import {
  AutoModel,
  AutoTokenizer,
  layer_norm,
  FeatureExtractionPipeline,
} from "@huggingface/transformers";
import { resolve } from "node:path";
import type { Embedding } from "../../../contracts/generated/activation-window.js";
import type {
  SearchCursor,
  MemoryRef,
} from "../../../contracts/generated/host-response.js";
import type { HostCommands } from "../../pi-worker/src/host-client.js";

export interface TextEmbeddings {
  document(text: string, signal?: AbortSignal): Promise<Embedding>;
  query(text: string, signal?: AbortSignal): Promise<Embedding>;
}

export class EmbeddingInputTooLong extends Error {
  readonly tokens: number;
  constructor(tokens: number) {
    super(
      `Embedding input has ${tokens} tokens; Nomic accepts at most 8192. Text was not truncated.`,
    );
    this.tokens = tokens;
  }
}

const model = "nomic-ai/nomic-embed-text-v1.5";
// Keep cached documents and queries on the same model release and encoder settings.
const revision = "e9b6763023c676ca8431644204f50c2b100d9aab";

export class NomicEmbeddings implements TextEmbeddings {
  private readonly extractor: FeatureExtractionPipeline;

  private constructor(extractor: FeatureExtractionPipeline) {
    this.extractor = extractor;
  }

  static async load(options: {
    cacheDir: string;
    allowDownload?: boolean;
  }): Promise<NomicEmbeddings> {
    const loading = {
      revision,
      cache_dir: options.cacheDir,
      local_files_only: !options.allowDownload,
    };
    // 4.3's tokenizer discovery drops loading options. A local directory prevents
    // that discovery from querying Hub metadata during offline startup.
    const source = options.allowDownload
      ? model
      : resolve(options.cacheDir, model, revision);
    const tokenizer = await AutoTokenizer.from_pretrained(source, loading);
    const encoder = await AutoModel.from_pretrained(source, {
      ...loading,
      dtype: "q8",
      device: "cpu",
      session_options: { intraOpNumThreads: 2 },
    });
    return new NomicEmbeddings(
      new FeatureExtractionPipeline({
        task: "feature-extraction",
        tokenizer,
        model: encoder,
      }),
    );
  }

  document(text: string, signal?: AbortSignal): Promise<Embedding> {
    return this.embed("search_document", text, signal);
  }

  query(text: string, signal?: AbortSignal): Promise<Embedding> {
    return this.embed("search_query", text, signal);
  }

  private async embed(
    prefix: string,
    text: string,
    signal?: AbortSignal,
  ): Promise<Embedding> {
    signal?.throwIfAborted();
    if (!text.trim()) throw new Error("Embedding input is empty");
    const input = `${prefix}: ${text}`;
    // The pipeline truncates by default, so check the complete prefixed input first.
    const tokens = this.extractor.tokenizer(input, {
      truncation: false,
      padding: false,
    }).input_ids.dims[1];
    if (tokens > 8192) throw new EmbeddingInputTooLong(tokens);
    const output = await this.extractor(input, { pooling: "mean" });
    signal?.throwIfAborted();
    const normalized = layer_norm(output, [output.dims[1]]).normalize(2, -1);
    const values = Array.from(normalized.data, Number);
    if (
      values.length !== 768 ||
      values.some((v) => !Number.isFinite(v)) ||
      !values.some((v) => v !== 0)
    )
      throw new Error("Nomic returned an invalid embedding");
    return {
      identity: {
        model,
        revision: `${revision}:q8:mean:layernorm:l2`,
        dimensions: 768,
        representation: "memory-content-v1",
      },
      values,
    };
  }

  async dispose(): Promise<void> {
    await this.extractor.dispose();
  }
}

/** One bounded page. Persist `after` with the administrator's scope and model configuration. */
export async function indexEmbeddingPage(
  host: HostCommands,
  embeddings: TextEmbeddings,
  options: { after?: SearchCursor; limit?: number; signal?: AbortSignal } = {},
): Promise<{
  indexed: number;
  skipped: { memory: MemoryRef; reason: string }[];
  after?: SearchCursor;
  exhausted: boolean;
}> {
  const limit = options.limit ?? 20;
  if (!Number.isInteger(limit) || limit < 1 || limit > 100)
    throw new Error("Embedding page limit must be between 1 and 100");
  options.signal?.throwIfAborted();
  const response = await host.request(
    { action: "embedding_inputs", after: options.after, limit },
    options.signal,
  );
  if (response.kind !== "embedding_inputs")
    throw new Error("Host did not return embedding inputs");
  let indexed = 0;
  const skipped: { memory: MemoryRef; reason: string }[] = [];
  for (const input of response.inputs) {
    options.signal?.throwIfAborted();
    let embedding: Embedding;
    try {
      embedding = await embeddings.document(input.text, options.signal);
    } catch (error) {
      if (!(error instanceof EmbeddingInputTooLong)) throw error;
      skipped.push({ memory: input.memory, reason: error.message });
      continue;
    }
    const saved = await host.request(
      { action: "save_embedding", reference: input.memory, embedding },
      options.signal,
    );
    if (saved.kind !== "done" || saved.affected !== 1)
      throw new Error("Host did not save the embedding");
    indexed++;
  }
  return {
    indexed,
    skipped,
    after: response.inputs.at(-1)?.cursor ?? options.after,
    exhausted: response.inputs.length < limit,
  };
}
