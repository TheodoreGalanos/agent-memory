import { readFile, writeFile } from "node:fs/promises";
import { randomUUID } from "node:crypto";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { expect, it, vi } from "vitest";
import { env } from "@huggingface/transformers";
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type {
  ActivationQuery,
  Embedding,
} from "../../../contracts/generated/activation-window.js";
import type { MemoryRef } from "../../../contracts/generated/host-response.js";
import { HostClient } from "../../pi-worker/src/host-client.js";
import { fenceFor } from "../../judgement/src/packet.js";
import {
  NomicEmbeddings,
  indexEmbeddingPage,
  EmbeddingInputTooLong,
} from "../src/embeddings.js";

it.skipIf(
  !process.env.MEMORY_TEST_EMBEDDINGS || !process.env.MEMORY_ACTIVATION_FIXTURE,
)(
  "indexes real Nomic vectors and retrieves paraphrased procedures through the Host",
  async () => {
    const f = JSON.parse(
      await readFile(process.env.MEMORY_ACTIVATION_FIXTURE!, "utf8"),
    ) as {
      assignment: Assignment;
      url: string;
      token: string;
      admin_token: string;
      method: MemoryRef;
      exception: MemoryRef;
      executable: MemoryRef;
    };
    const worker = new HostClient(f.url, f.token);
    const admin = new HostClient(f.url, f.admin_token);
    const started = performance.now();
    // Download is a separate explicit command; qualification itself works offline.
    const fetch = vi
      .spyOn(env, "fetch")
      .mockRejectedValue(new Error("Model loading must stay offline"));
    let embeddings: NomicEmbeddings;
    try {
      embeddings = await NomicEmbeddings.load({
        cacheDir: process.env.MEMORY_EMBEDDING_CACHE ?? ".runtime/embeddings",
      });
      expect(fetch).not.toHaveBeenCalled();
    } finally {
      fetch.mockRestore();
    }
    const loadedMs = performance.now() - started;
    try {
      const page = await indexEmbeddingPage(admin, embeddings, { limit: 2 });
      expect(page.indexed).toBe(2);
      expect(page.exhausted).toBe(false);
      const rest = await indexEmbeddingPage(admin, embeddings, {
        after: page.after,
      });
      expect(rest.indexed).toBe(1);
      expect(rest.exhausted).toBe(true);
      expect([...page.skipped, ...rest.skipped]).toEqual([]);
      expect(
        (await indexEmbeddingPage(admin, embeddings, { after: rest.after }))
          .indexed,
      ).toBe(0);
      const cli = await promisify(execFile)(
        process.execPath,
        [
          "--experimental-strip-types",
          "scripts/embeddings.ts",
          "index",
          JSON.stringify(rest.after),
        ],
        {
          env: {
            ...process.env,
            MEMORY_HOST_URL: f.url,
            MEMORY_HOST_TOKEN: f.admin_token,
          },
        },
      );
      expect(JSON.parse(cli.stdout)).toMatchObject({
        indexed: 0,
        exhausted: true,
        after: rest.after,
      });
      const cases = [
        {
          question: "Depressurise rotating equipment",
          expected: f.method.memory_id,
        },
        {
          question: "Compute torsional force",
          expected: f.executable.memory_id,
        },
      ];
      const checks = [];
      for (const c of cases) {
        const query: ActivationQuery = {
          scope: f.assignment.job.spec.brief.scope,
          question: c.question,
          task_context: "",
          entities: [],
          exact: [],
          families: ["procedure"],
          existing: [],
          scan_limit: 100,
          candidate_limit: 1,
          traversal_limit: 20,
          context_bytes: 20000,
        };
        const retrieve = async (vector?: Embedding) => {
          const response = await worker.request({
            action: "activate",
            fence: fenceFor(f.assignment),
            id: randomUUID(),
            query: { ...query, vector },
          });
          if (response.kind !== "activation_window")
            throw new Error("Expected activation window");
          return response.window;
        };
        const lexical = await retrieve();
        expect(lexical.candidates).toEqual([]);
        const time = performance.now();
        const vector = await embeddings.query(c.question);
        expect(vector.values).toHaveLength(768);
        expect(Math.hypot(...vector.values)).toBeCloseTo(1, 5);
        const semantic = await retrieve(vector);
        const match = semantic.candidates.find(
          (candidate) => candidate.memory.reference.memory_id === c.expected,
        );
        expect(match?.channels).toContain("vector");
        expect(
          semantic.coverage.some((s) =>
            s.includes("lack a matching current embedding"),
          ),
        ).toBe(false);
        checks.push({
          question: c.question,
          lexicalMatches: lexical.candidates.length,
          expectedRetrieved: !!match,
          queryAndRetrievalMs: Math.round(performance.now() - time),
        });
      }
      // Exercise the ONNX model beyond its 2048-token training length, and reject over-limit input.
      const long = await embeddings.document("valve ".repeat(2200));
      expect(long.values.every(Number.isFinite)).toBe(true);
      await expect(embeddings.document("valve ".repeat(8192))).rejects.toThrow(
        EmbeddingInputTooLong,
      );
      if (process.env.MEMORY_EMBEDDING_REPORT)
        await writeFile(
          process.env.MEMORY_EMBEDDING_REPORT,
          JSON.stringify(
            {
              model: long.identity,
              runtime: "Transformers.js 4.3.0 / CPU",
              indexed: page.indexed + rest.indexed,
              loadedMs: Math.round(loadedMs),
              totalMs: Math.round(performance.now() - started),
              checks,
              scope:
                "Two authored paraphrase smoke cases, not a held-out retrieval benchmark. No semantic judgement provider called.",
            },
            null,
            2,
          ) + "\n",
        );
    } finally {
      await embeddings.dispose();
    }
  },
  180_000,
);
