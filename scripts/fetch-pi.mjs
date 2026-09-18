import {
  access,
  mkdtemp,
  mkdir,
  rename,
  rm,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { execFileSync } from "node:child_process";
import { readFile } from "node:fs/promises";

const { pi } = JSON.parse(
  await readFile(new URL("../contracts/compatibility.json", import.meta.url)),
);
const parent = new URL("../.upstream/", import.meta.url);
const destination = new URL(`pi/`, parent);
try {
  await access(new URL("package.json", destination));
  console.log(
    "Pi source is present. Remove .upstream/pi before changing its revision.",
  );
  process.exit(0);
} catch (error) {
  if (error.code !== "ENOENT") throw error;
}
const temp = await mkdtemp(join(tmpdir(), "memory-pi-"));
try {
  const response = await fetch(
    `https://codeload.github.com/${pi.repository}/tar.gz/${pi.commit}`,
  );
  if (!response.ok) throw new Error(`Pi download failed: ${response.status}`);
  const archive = join(temp, "pi.tar.gz");
  await writeFile(archive, Buffer.from(await response.arrayBuffer()));
  const source = join(temp, "source");
  await mkdir(source);
  execFileSync("tar", ["-xzf", archive, "-C", source, "--strip-components=1"]);
  await mkdir(parent, { recursive: true });
  await rename(source, destination);
  console.log(`Fetched Pi ${pi.commit}`);
} finally {
  await rm(temp, { recursive: true, force: true });
}
