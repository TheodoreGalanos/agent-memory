import {
  NomicEmbeddings,
  indexEmbeddingPage,
} from "../packages/activation/src/embeddings.ts";
import { HostClient } from "../packages/pi-worker/src/host-client.ts";

const [command, argument] = process.argv.slice(2);
if (!["download", "query", "index"].includes(command))
  throw new Error(
    "Usage: npm run embeddings -- download | query TEXT | index [CURSOR_JSON]",
  );
if (command === "query" && !argument) throw new Error("Supply query text");
const url = process.env.MEMORY_HOST_URL;
const token = process.env.MEMORY_HOST_TOKEN;
if (command === "index" && (!url || !token))
  throw new Error(
    "Indexing needs MEMORY_HOST_URL and an administrator MEMORY_HOST_TOKEN",
  );
const after =
  command === "index" && argument ? JSON.parse(argument) : undefined;
const embeddings = await NomicEmbeddings.load({
  cacheDir: process.env.MEMORY_EMBEDDING_CACHE ?? ".runtime/embeddings",
  allowDownload: command === "download",
});
try {
  if (command === "download") {
    const sample = await embeddings.query("Check local embedding inference");
    console.log(JSON.stringify({ ready: true, identity: sample.identity }));
  } else if (command === "query") {
    console.log(JSON.stringify(await embeddings.query(argument)));
  } else {
    // One page per invocation keeps the command bounded; pass `after` to continue.
    console.log(
      JSON.stringify(
        await indexEmbeddingPage(new HostClient(url!, token!), embeddings, {
          after,
        }),
      ),
    );
  }
} finally {
  await embeddings.dispose();
}
