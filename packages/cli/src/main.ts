#!/usr/bin/env node
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { resolve, join, dirname } from "node:path";
import { randomUUID, randomBytes } from "node:crypto";
import { parseArgs } from "node:util";
import type { HostRequest } from "../../../contracts/generated/host-request.js";

const help = `Memory API CLI

npm run memory -- init DIRECTORY [--port 7331]
npm run memory -- enable-pool DIRECTORY
npm run memory -- COMMAND --config CLIENT.json [options]

Commands:
  identity                     Show the authenticated actor and scope
  browse [--file QUERY.json]    Read scoped current memories (entity/time filters)
  history --id UUID            Read versions; --after REVISION, --limit N
  show --file REFERENCE.json    Inspect a memory and its relations
  task --id UUID               Inspect a job, context and coverage
  exploration --id UUID        Inspect a temporary exploration
  decisions --id JOB_UUID       Inspect prepared decisions
  changes                      Read changes; --after CURSOR, --limit N
  notifications                Poll notifications; --after CURSOR, --limit N
  controls                     Inspect operator controls
  mutate --file MUTATION.json   Make a user/operator change; --request-id UUID
  call --file REQUEST.json      Send any documented HostRequest (including ingest_source)
  backup DIRECTORY --config ADMIN.json     Write a consistent backup into a new directory
  restore BACKUP_DIR NEW_DIR --host ORIGINAL/host.json [--registry DELETIONS.json] [--port N]

--out FILE also saves the JSON response. Reuse --request-id for mutation retries.
init creates private local host/client/admin configurations; it starts no services.
enable-pool DIRECTORY adds a pool credential, worker_pool and worker.json so the Host runs a worker pool.
Schemas: GET /schemas/host-request.json and /openapi.json on the running Host.
`;
async function main() {
  const { values, positionals } = parseArgs({ allowPositionals: true, options: {
    config: { type: "string" }, file: { type: "string" }, id: { type: "string" },
    after: { type: "string" }, limit: { type: "string" }, out: { type: "string" },
    "after-manifest": { type: "string" }, "request-id": { type: "string" }, port: { type: "string" }, help: { type: "boolean" },
    host: { type: "string" }, registry: { type: "string" },
  }});
  const command = positionals[0];
  if (values.help || !command) { console.log(help); return; }
  if (command === "init") {
    if (!positionals[1]) throw new Error("init requires a new directory");
    const directory = resolve(positionals[1]);
    const port = Number(values.port ?? 7331);
    if (!Number.isInteger(port) || port < 1024 || port > 65535) throw new Error("Port must be 1024–65535");
    await mkdir(directory, { mode: 0o700 });
    const tenant_id = randomUUID(), actor_id = randomUUID();
    const scope = { project_id: randomUUID(), user_id: null, task_id: null, entity_ids: [], source_versions: [] };
    const endpoint = `http://127.0.0.1:${port}`;
    const clientToken = randomBytes(32).toString("hex"), adminToken = randomBytes(32).toString("hex");
    const expires_at = new Date(Date.now() + 30 * 86400000).toISOString();
    const files = {
      "host.json": { listen: `127.0.0.1:${port}`, database_url: `sqlite://${join(directory, "memory.db")}?mode=rwc`, artifact_root: join(directory,"artifacts"), credentials: [
        { token: clientToken, tenant_id, actor_id, scope, role: {kind:"client"}, expires_at },
        { token: adminToken, tenant_id, actor_id: randomUUID(), scope: { ...scope, project_id:null }, role: {kind:"administrator"}, expires_at },
      ] },
      "client.json": { endpoint, token:clientToken }, "admin.json": { endpoint, token:adminToken },
    };
    for (const [name,data] of Object.entries(files)) await writeFile(join(directory,name), JSON.stringify(data,null,2)+"\n",{mode:0o600,flag:"wx"});
    console.log(JSON.stringify({directory,host:join(directory,"host.json"),client:join(directory,"client.json"),admin:join(directory,"admin.json"),expires_at},null,2));
    return;
  }
  if (command === "enable-pool") {
    if (!positionals[1]) throw new Error("enable-pool requires an initialized directory");
    const directory = resolve(positionals[1]), hostPath = join(directory, "host.json");
    const host = JSON.parse(await readFile(hostPath, "utf8")) as { credentials: Record<string, unknown>[]; worker_pool?: unknown };
    const administrator = host.credentials.find((c) => (c.role as {kind:string}).kind === "administrator") as { tenant_id: string; scope: Record<string, unknown>; expires_at: string } | undefined;
    if (!administrator) throw new Error("host.json has no administrator credential");
    // The Host starts one worker pool child from this checkout; the pool token stays in host.json.
    const root = process.cwd();
    if (!host.credentials.some((c) => (c.role as {kind:string}).kind === "pool"))
      host.credentials.push({ token: randomBytes(32).toString("hex"), tenant_id: administrator.tenant_id, actor_id: randomUUID(), scope: administrator.scope, role: { kind: "pool", classes: ["interactive", "deferred"] }, expires_at: administrator.expires_at });
    host.worker_pool = { command: process.execPath, args: ["--import", join(root, "scripts/ts-loader.mjs"), join(root, "packages/supervisor/src/main.ts"), join(directory, "worker.json")], working_directory: root, restart_delay_seconds: 5 };
    const worker = { env_file: join(root, ".env"), sessions_directory: join(directory, "pool"), concurrency: { interactive: 1, deferred: 1 }, poll_ms: 1000, lease_seconds: 120,
      task_model: { provider: "azure-openai-responses", model: "gpt-5.4-mini" }, reference_model: { provider: "azure-openai-responses", model: "gpt-4.1-mini" }, jev: { model: "jev-latest" }, judgement_mode: "shadow" };
    await writeFile(join(directory, "worker.json"), JSON.stringify(worker, null, 2) + "\n", { mode: 0o600, flag: "wx" });
    await writeFile(hostPath, JSON.stringify(host, null, 2) + "\n", { mode: 0o600 });
    console.log(JSON.stringify({ host: hostPath, worker: join(directory, "worker.json"), note: "Restart the Host to start the worker pool" }, null, 2));
    return;
  }
  if (command === "backup") {
    if (!values.config || !positionals[1]) throw new Error("backup requires --config ADMIN.json and a new DIRECTORY");
    const directory = resolve(positionals[1]);
    const config = JSON.parse(await readFile(values.config, "utf8")) as { endpoint: string; token: string };
    const response = await fetch(new URL("/v1/commands", config.endpoint), { method: "POST", headers: { authorization: `Bearer ${config.token}`, "content-type": "application/json" }, body: JSON.stringify({ action: "backup", directory }), signal: AbortSignal.timeout(600000), redirect: "error" });
    const body = await response.text();
    if (!response.ok) throw new Error(`HTTP ${response.status}: ${body}`);
    process.stdout.write(JSON.stringify(JSON.parse(body), null, 2) + "\n");
    return;
  }
  if (command === "restore") {
    // restore BACKUP_DIR NEW_DIR --host ORIGINAL/host.json [--registry DELETIONS.json] [--port N]
    const [, backupArg, targetArg] = positionals;
    if (!backupArg || !targetArg || !values.host) throw new Error("restore requires BACKUP_DIR NEW_DIR --host ORIGINAL_HOST.json");
    const backup = resolve(backupArg), target = resolve(targetArg);
    const manifest = JSON.parse(await readFile(join(backup, "manifest.json"), "utf8")) as { database: { file: string; sha256: string }; artifacts: { root: string } };
    const original = JSON.parse(await readFile(resolve(values.host), "utf8")) as Record<string, unknown>;
    const port = Number(values.port ?? new URL(`http://${String(original.listen)}`).port);
    if (!Number.isInteger(port) || port < 1024 || port > 65535) throw new Error("Port must be 1024–65535");
    await mkdir(target, { mode: 0o700 });
    // Restore into isolation: the database copy is verified, then the deletion registry is applied
    // by the Host at startup before any credential can read (WP13 restore barrier).
    const { createHash } = await import("node:crypto");
    const database = await readFile(join(backup, manifest.database.file));
    if (createHash("sha256").update(database).digest("hex") !== manifest.database.sha256) throw new Error("Backup database checksum does not match its manifest");
    await writeFile(join(target, "memory.db"), database, { mode: 0o600, flag: "wx" });
    const { cp } = await import("node:fs/promises");
    await cp(join(backup, manifest.artifacts.root), join(target, "artifacts"), { recursive: true });
    const registry = values.registry ? resolve(values.registry) : join(backup, "deletions.json");
    const host = { ...original, listen: `127.0.0.1:${port}`, database_url: `sqlite://${join(target, "memory.db")}?mode=rwc`, artifact_root: join(target, "artifacts"), restore_registry: registry };
    delete (host as { worker_pool?: unknown }).worker_pool;
    await writeFile(join(target, "host.json"), JSON.stringify(host, null, 2) + "\n", { mode: 0o600, flag: "wx" });
    for (const name of ["client.json", "admin.json"]) {
      try {
        const file = JSON.parse(await readFile(join(dirname(resolve(values.host)), name), "utf8")) as { token: string };
        await writeFile(join(target, name), JSON.stringify({ endpoint: `http://127.0.0.1:${port}`, token: file.token }, null, 2) + "\n", { mode: 0o600, flag: "wx" });
      } catch { /* The original instance may keep its credentials elsewhere. */ }
    }
    console.log(JSON.stringify({ directory: target, host: join(target, "host.json"), registry, note: "Start the Host with this host.json; the pool is not enabled on a restored instance until you review it" }, null, 2));
    return;
  }
  if (!values.config) throw new Error("--config CLIENT.json is required");
  const config = JSON.parse(await readFile(values.config,"utf8")) as {endpoint:string,token:string};
  const endpoint = new URL(config.endpoint);
  if (!['http:','https:'].includes(endpoint.protocol) || endpoint.username || endpoint.password) throw new Error("Configuration needs an HTTP(S) endpoint without embedded credentials");
  if (endpoint.protocol === 'http:' && !['127.0.0.1','localhost','[::1]'].includes(endpoint.hostname)) throw new Error("Remote endpoints require HTTPS");
  const input = async () => { if (!values.file) throw new Error("--file is required"); return JSON.parse(await readFile(values.file,"utf8")); };
  const id = () => { if (!values.id) throw new Error("--id is required"); return values.id; };
  const integer = (value:string|undefined,fallback:number) => { const result=Number(value??fallback); if (!Number.isSafeInteger(result)||result<0) throw new Error("Cursor and limit must be non-negative integers");return result; };
  const after=integer(values.after,0),limit=integer(values.limit,50);
  let request:HostRequest;
  switch(command) {
    case "call": request=await input(); break;
    case "identity": case "controls": request={action:"user",request:{action:command}}; break;
    case "browse": request={action:"user",request:{action:"browse",query: values.file ? await input() : {include_inactive:false,limit}}}; break;
    case "history": request={action:"user",request:{action:"history",memory_id:id(),after_revision:after,limit}}; break;
    case "show": request={action:"user",request:{action:"inspect_memory",reference:await input()}}; break;
    case "task": request={action:"user",request:{action:"inspect_task",job_id:id(),after_manifest:values["after-manifest"]}}; break;
    case "decisions": request={action:"user",request:{action:"decisions",job_id:id()}}; break;
    case "exploration": request={action:"user",request:{action:"inspect_exploration",id:id()}}; break;
    case "changes": case "notifications": request={action:"user",request:{action:command,after,limit}}; break;
    case "mutate": request={action:"user",request:{action:"mutate",request_id:values["request-id"]??randomUUID(),mutation:await input()}}; break;
    default: throw new Error(`Unknown command: ${command}\n${help}`);
  }
  const response=await fetch(new URL("/v1/commands",endpoint),{method:"POST",headers:{authorization:`Bearer ${config.token}`,"content-type":"application/json"},body:JSON.stringify(request),signal:AbortSignal.timeout(30000),redirect:"error"});
  const body=await response.text();
  if(!response.ok) throw new Error(`HTTP ${response.status}: ${body}`);
  const formatted=JSON.stringify(JSON.parse(body),null,2)+"\n";
  if(values.out) await writeFile(values.out,formatted,{mode:0o600});
  process.stdout.write(formatted);
}
main().catch((error:unknown)=>{ console.error(error instanceof Error ? error.message : String(error));process.exitCode=1; });
