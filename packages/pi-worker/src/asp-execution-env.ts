import { spawn } from "node:child_process";
import { mkdtemp, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, isAbsolute } from "node:path";
import { isDeepStrictEqual } from "node:util";
import { randomUUID } from "node:crypto";
import { setTimeout as delay } from "node:timers/promises";
import {
  FileError,
  ExecutionError,
  truncateHead,
  truncateTail,
  type Context,
  type ExecutionEnv,
  type FileInfo,
  type FileErrorCode,
  type ShellExecOptions,
  type ShellExecResult,
  type Result,
} from "@earendil-works/pi-agent-core";

export interface AspDescriptor {
  version: "0";
  transport: "ssh";
  connection: {
    host: string;
    port?: number;
    user?: string;
    identity?: { file: string };
    host_key: string;
  };
  workspace: string;
}
export interface SandboxBinding {
  sandbox_id: string;
  generation: string;
  session_id: string;
  epoch: number;
  expires_at: number;
  descriptor: AspDescriptor;
}
interface RemoteStatus {
  output_path?: string;
  state: "starting" | "running" | "finished" | "outcome_unknown";
  exit_code?: number;
  reason?: string;
  bytes?: number;
  process_group_gone?: boolean;
}
type LineReaderResult = Awaited<ReturnType<ExecutionEnv["openTextLineReader"]>>;
type TextLineReader = Extract<LineReaderResult, { ok: true }>["value"];
const maxTransfer = 1024 * 1024;

function validate(binding: SandboxBinding, expected: SandboxBinding) {
  const descriptor = binding.descriptor;
  if (
    binding.sandbox_id !== expected.sandbox_id ||
    binding.generation !== expected.generation ||
    binding.session_id !== expected.session_id ||
    binding.epoch !== expected.epoch ||
    binding.expires_at !== expected.expires_at ||
    !isDeepStrictEqual(descriptor, expected.descriptor)
  )
    throw new Error(
      "Sandbox descriptor does not match the trusted allocation receipt",
    );
  if (
    descriptor.version !== "0" ||
    descriptor.transport !== "ssh" ||
    !descriptor.workspace.startsWith("/") ||
    descriptor.workspace.includes("\0")
  )
    throw new Error("Unsupported ASP descriptor");
  const c = descriptor.connection;
  if (
    Object.keys(descriptor).some(
      (key) =>
        ![
          "$schema",
          "version",
          "transport",
          "connection",
          "workspace",
        ].includes(key),
    ) ||
    Object.keys(c).some(
      (key) => !["host", "port", "user", "identity", "host_key"].includes(key),
    ) ||
    (c.identity && Object.keys(c.identity).some((key) => key !== "file"))
  )
    throw new Error("Unsupported ASP configuration field");
  if (
    !/^[A-Za-z0-9][A-Za-z0-9.:-]*$/.test(c.host) ||
    !/^[A-Za-z0-9_.-]+$/.test(c.user ?? "") ||
    !Number.isInteger(c.port ?? 22) ||
    (c.port ?? 22) < 1 ||
    (c.port ?? 22) > 65535 ||
    !/^(ssh-ed25519|ssh-rsa|ecdsa-sha2-nistp256) [A-Za-z0-9+/]+={0,3}$/.test(
      c.host_key,
    )
  )
    throw new Error("Invalid pinned SSH connection");
  if (
    c.identity &&
    (!isAbsolute(c.identity.file) || c.identity.file.includes("\0"))
  )
    throw new Error("SSH identity must be an absolute trusted file reference");
  if (
    !Number.isSafeInteger(binding.epoch) ||
    binding.epoch < 1 ||
    Date.now() >= binding.expires_at
  )
    throw new Error("Sandbox binding is expired or invalid");
}

/** OpenSSH only. No local execution fallback and no workspace descriptor discovery. */
export class AspExecutionEnv implements ExecutionEnv {
  readonly cwd: string;
  private closed = false;
  private closing = false;
  private executions = new Set<
    Promise<Result<ShellExecResult, ExecutionError>>
  >();
  private constructor(
    private readonly binding: SandboxBinding,
    private readonly directory: string,
    private readonly helperPath: string,
  ) {
    this.cwd = binding.descriptor.workspace;
  }
  static async connect(
    binding: SandboxBinding,
    trustedReceipt: SandboxBinding,
    helperPath = "/opt/memory/asp_helper.py",
  ) {
    validate(binding, trustedReceipt);
    if (!/^\/[A-Za-z0-9_./-]+$/.test(helperPath))
      throw new Error("Invalid trusted helper path");
    const directory = await mkdtemp(join(tmpdir(), "memory-asp-"));
    await writeFile(
      join(directory, "known_hosts"),
      `memory-sandbox ${binding.descriptor.connection.host_key}\n`,
      { mode: 0o600 },
    );
    return new AspExecutionEnv(structuredClone(binding), directory, helperPath);
  }
  renewBinding(binding: SandboxBinding, trustedReceipt: SandboxBinding) {
    validate(binding, trustedReceipt);
    if (
      binding.sandbox_id !== this.binding.sandbox_id ||
      binding.generation !== this.binding.generation ||
      binding.session_id !== this.binding.session_id ||
      binding.epoch !== this.binding.epoch ||
      !isDeepStrictEqual(binding.descriptor, this.binding.descriptor)
    )
      throw new Error("Renewal changed the sandbox identity or assignment");
    this.binding.expires_at = binding.expires_at;
  }
  private async rpc<T>(
    command: Record<string, unknown>,
    signal?: AbortSignal,
  ): Promise<T> {
    if (this.closed || Date.now() >= this.binding.expires_at)
      throw new FileError(
        "permission_denied",
        "Sandbox binding is closed or expired",
      );
    const { descriptor, ...binding } = this.binding;
    const data = JSON.stringify({ ...command, ...binding });
    if (Buffer.byteLength(data) > 2 * maxTransfer)
      throw new FileError("invalid", "Request exceeds transfer limit");
    const connection = descriptor.connection;
    const args = [
      "-F",
      "/dev/null",
      "-T",
      "-p",
      String(connection.port ?? 22),
      "-l",
      connection.user!,
      "-o",
      "BatchMode=yes",
      "-o",
      "StrictHostKeyChecking=yes",
      "-o",
      "HostKeyAlias=memory-sandbox",
      "-o",
      `UserKnownHostsFile=${join(this.directory, "known_hosts")}`,
      "-o",
      "GlobalKnownHostsFile=/dev/null",
      "-o",
      "IdentityAgent=none",
      "-o",
      "IdentitiesOnly=yes",
      "-o",
      "ClearAllForwardings=yes",
      "-o",
      "ForwardAgent=no",
      "-o",
      "ForwardX11=no",
      "-o",
      "PermitLocalCommand=no",
      "-o",
      "ProxyCommand=none",
      "-o",
      "ProxyJump=none",
      "-o",
      "ControlMaster=no",
      "-o",
      "ControlPath=none",
      "-o",
      "ConnectTimeout=5",
      ...(connection.identity ? ["-i", connection.identity.file] : []),
      connection.host,
      `python3 ${this.helperPath}`,
    ];
    const combined = AbortSignal.any([
      AbortSignal.timeout(15_000),
      ...(signal ? [signal] : []),
    ]);
    const response = await new Promise<string>((resolve, reject) => {
      const child = spawn("ssh", args, {
        stdio: ["pipe", "pipe", "pipe"],
        signal: combined,
      });
      const output: Buffer[] = [];
      let bytes = 0;
      let failure: Error | undefined;
      child.stdout.on("data", (chunk: Buffer) => {
        bytes += chunk.length;
        if (bytes > 2 * maxTransfer) {
          failure = new Error("SSH response exceeds transfer limit");
          child.kill();
        } else output.push(chunk);
      });
      // Drain diagnostics without exposing identity references or accumulating an unbounded buffer.
      child.stderr.on("data", () => {});
      child.stdin.on("error", () => {});
      child.once("error", reject);
      child.once("close", (code) =>
        failure
          ? reject(failure)
          : code === 0
            ? resolve(Buffer.concat(output).toString("utf8"))
            : reject(new Error("Sandbox SSH connection unavailable")),
      );
      child.stdin.end(data);
    });
    const decoded = JSON.parse(response) as {
      ok: boolean;
      value: T;
      error: { code: FileErrorCode; message: string };
    };
    if (!decoded.ok)
      throw new FileError(decoded.error.code, decoded.error.message);
    return decoded.value;
  }
  async interpreter(
    request: Record<string, unknown>,
    context: Context,
  ): Promise<import("./interpreter-client.js").InterpreterReply> {
    return this.rpc({ op: "interpreter", request }, context.abortSignal);
  }
  private async file<T>(
    command: Record<string, unknown>,
    context: Context,
  ): Promise<Result<T, FileError>> {
    try {
      return {
        ok: true,
        value: await this.rpc<T>(command, context.abortSignal),
      };
    } catch (error) {
      return {
        ok: false,
        error:
          error instanceof FileError
            ? error
            : new FileError(
                context.abortSignal?.aborted ? "aborted" : "unknown",
                "Sandbox operation unavailable",
              ),
      };
    }
  }
  absolutePath(path: string, context: Context) {
    return this.file<string>({ op: "absolute", path }, context);
  }
  joinPath(parts: string[], context: Context) {
    return this.file<string>({ op: "join", parts }, context);
  }
  async readBinaryFile(
    path: string,
    context: Context,
  ): Promise<Result<Uint8Array, FileError>> {
    const read = await this.file<{ data: string }>(
      { op: "read", path },
      context,
    );
    return read.ok
      ? { ok: true, value: Buffer.from(read.value.data, "base64") }
      : read;
  }
  async readTextFile(
    path: string,
    context: Context,
  ): Promise<Result<string, FileError>> {
    const read = await this.readBinaryFile(path, context);
    return read.ok
      ? { ok: true, value: Buffer.from(read.value).toString("utf8") }
      : read;
  }
  async openTextLineReader(
    path: string,
    context: Context,
  ): Promise<Result<TextLineReader, FileError>> {
    const metadata = await this.file<FileInfo>({ op: "info", path }, context);
    if (!metadata.ok) return metadata;
    let offset = 0;
    let buffered = Buffer.alloc(0);
    let eof = false;
    let closed = false;
    return {
      ok: true,
      value: {
        readLine: async (readContext) => {
          if (closed)
            return {
              ok: false,
              error: new FileError("invalid", "Line reader is closed"),
            };
          while (true) {
            const newline = buffered.indexOf(10);
            if (newline >= 0 || eof) {
              if (!buffered.length) return { ok: true, value: undefined };
              const end = newline < 0 ? buffered.length : newline;
              const text = buffered
                .subarray(0, end)
                .toString("utf8")
                .replace(/\r$/, "");
              buffered = buffered.subarray(end + (newline >= 0 ? 1 : 0));
              return { ok: true, value: { text, terminated: newline >= 0 } };
            }
            if (buffered.length >= maxTransfer)
              return {
                ok: false,
                error: new FileError("invalid", "Line exceeds 1 MiB"),
              };
            const chunk = await this.file<{ data: string; eof: boolean }>(
              {
                op: "read_chunk",
                path,
                offset,
                limit: Math.min(65536, maxTransfer - buffered.length),
              },
              readContext,
            );
            if (!chunk.ok) return chunk;
            const data = Buffer.from(chunk.value.data, "base64");
            offset += data.length;
            buffered = Buffer.concat([buffered, data]);
            eof = chunk.value.eof;
          }
        },
        close: async () => {
          closed = true;
          buffered = Buffer.alloc(0);
        },
      },
    };
  }
  async readTextLines(
    path: string,
    options: { maxLines?: number } | undefined,
    context: Context,
  ): Promise<Result<string[], FileError>> {
    const opened = await this.openTextLineReader(path, context);
    if (!opened.ok) return opened;
    const lines: string[] = [];
    let bytes = 0;
    try {
      while (lines.length < (options?.maxLines ?? Number.POSITIVE_INFINITY)) {
        const line = await opened.value.readLine(context);
        if (!line.ok) return line;
        if (!line.value) break;
        bytes +=
          Buffer.byteLength(line.value.text) + (line.value.terminated ? 1 : 0);
        if (bytes > maxTransfer)
          return {
            ok: false,
            error: new FileError("invalid", "Lines exceed bounded read limit"),
          };
        lines.push(line.value.text);
      }
      return { ok: true, value: lines };
    } finally {
      await opened.value.close(context);
    }
  }
  writeFile(path: string, content: string | Uint8Array, context: Context) {
    return this.file<void>(
      { op: "write", path, data: Buffer.from(content).toString("base64") },
      context,
    );
  }
  appendFile(path: string, content: string | Uint8Array, context: Context) {
    return this.file<void>(
      { op: "append", path, data: Buffer.from(content).toString("base64") },
      context,
    );
  }
  renameFile(sourcePath: string, destinationPath: string, context: Context) {
    return this.file<void>(
      { op: "rename", path: sourcePath, destination: destinationPath },
      context,
    );
  }
  fileInfo(path: string, context: Context) {
    return this.file<FileInfo>({ op: "info", path }, context);
  }
  listDir(path: string, context: Context) {
    return this.file<FileInfo[]>({ op: "list", path }, context);
  }
  canonicalPath(path: string, context: Context) {
    return this.file<string>({ op: "canonical", path }, context);
  }
  exists(path: string, context: Context) {
    return this.file<boolean>({ op: "exists", path }, context);
  }
  createDir(
    path: string,
    options: { recursive?: boolean } | undefined,
    context: Context,
  ) {
    return this.file<void>({ op: "mkdir", path, ...options }, context);
  }
  remove(
    path: string,
    options: { recursive?: boolean; force?: boolean } | undefined,
    context: Context,
  ) {
    return this.file<void>({ op: "remove", path, ...options }, context);
  }
  createTempDir(prefix: string | undefined, context: Context) {
    return this.file<string>(
      { op: "temp_dir", prefix: prefix ?? "tmp-" },
      context,
    );
  }
  createTempFile(
    options: { prefix?: string; suffix?: string } | undefined,
    context: Context,
  ) {
    return this.file<string>({ op: "temp_file", ...options }, context);
  }
  async exec(
    command: string,
    options: ShellExecOptions | undefined,
    context: Context,
  ): Promise<Result<ShellExecResult, ExecutionError>> {
    if (this.closing || this.closed)
      return {
        ok: false,
        error: new ExecutionError("aborted", "Sandbox adapter is closing"),
      };
    const execution = this.execute(command, options, context);
    this.executions.add(execution);
    try {
      return await execution;
    } finally {
      this.executions.delete(execution);
    }
  }
  private async execute(
    command: string,
    options: ShellExecOptions | undefined,
    context: Context,
  ): Promise<Result<ShellExecResult, ExecutionError>> {
    const executionId = randomUUID();
    const deadline = Math.min(
      this.binding.expires_at,
      Date.now() + (options?.timeout ?? 300) * 1000,
    );
    try {
      await this.rpc(
        {
          op: "start",
          execution_id: executionId,
          command,
          cwd: options?.cwd ?? this.cwd,
          env: options?.env ?? {},
          inherit_env: options?.inheritEnv ?? true,
          timeout_ms: Math.max(1, deadline - Date.now()),
          max_output_bytes: 16 * maxTransfer,
        },
        context.abortSignal,
      );
      let status: RemoteStatus;
      let cancellationSent = false;
      while (true) {
        if (
          !cancellationSent &&
          (this.closing ||
            context.abortSignal?.aborted ||
            Date.now() >= deadline)
        ) {
          await this.rpc({ op: "cancel", execution_id: executionId });
          cancellationSent = true;
        }
        status = await this.rpc<RemoteStatus>({
          op: "status",
          execution_id: executionId,
        });
        if (status.state === "finished" || status.state === "outcome_unknown")
          break;
        if (Date.now() > deadline + 10_000)
          throw new Error("Remote terminal state unavailable");
        await delay(100);
      }
      if (status.state !== "finished" || !status.process_group_gone)
        throw new Error("Remote process group outcome unknown");
      if (
        cancellationSent ||
        status.reason === "cancelled" ||
        status.reason === "timeout"
      )
        return {
          ok: false,
          error: new ExecutionError(
            this.closing || context.abortSignal?.aborted
              ? "aborted"
              : "timeout",
            `Remote process group terminated; execution ${executionId}`,
          ),
        };
      if (status.reason)
        throw new Error(`Remote execution stopped: ${status.reason}`);
      const limits = options?.capture?.limits ?? {
        maxBytes: 50 * 1024,
        maxLines: 2000,
        retain: "tail" as const,
      };
      const maxBytes = Math.max(1, Math.min(maxTransfer, limits.maxBytes));
      const maxLines = Math.max(1, Math.min(4096, limits.maxLines));
      let retained = "";
      let totalBytes = 0;
      let totalLines = 0;
      let lastWasNewline = true;
      let truncatedBy: "bytes" | "lines" | null = null;
      const truncate = limits.retain === "head" ? truncateHead : truncateTail;
      let view = truncate("", { maxBytes, maxLines });
      const decoder = new TextDecoder();
      const keep = (text: string) => {
        if (limits.retain === "head" && truncatedBy) return;
        view = truncate(retained + text, { maxBytes, maxLines });
        retained = view.content;
        truncatedBy = view.truncatedBy ?? truncatedBy;
      };
      for await (const bytes of this.exportOutput(executionId, context)) {
        totalBytes += bytes.length;
        for (const byte of bytes) if (byte === 10) totalLines++;
        if (bytes.length) lastWasNewline = bytes[bytes.length - 1] === 10;
        keep(decoder.decode(bytes, { stream: true }));
      }
      keep(decoder.decode());
      if (totalBytes && !lastWasNewline) totalLines++;
      const { content: _, ...truncation } = view;
      const metadata = {
        truncation: {
          ...truncation,
          truncated: truncatedBy !== null,
          truncatedBy,
          totalBytes,
          totalLines,
        },
        ...(truncatedBy && options?.capture?.spill
          ? {
              spillPath: await this.rpc<string>(
                { op: "spill", execution_id: executionId },
                context.abortSignal,
              ),
            }
          : {}),
      };
      options?.onUpdate?.(
        { kind: "replace", output: { text: retained, ...metadata } },
        context,
      );
      return {
        ok: true,
        value: { exitCode: status.exit_code ?? -1, ...metadata },
      };
    } catch (error) {
      return {
        ok: false,
        error: new ExecutionError(
          "unknown",
          `Sandbox execution ${executionId} is unresolved: ${error instanceof Error ? error.message : "unavailable"}`,
        ),
      };
    }
  }
  /** Stream full retained output to the host artifact publisher before allocation teardown. */
  async *exportOutput(
    executionId: string,
    context: Context,
  ): AsyncGenerator<Uint8Array> {
    let offset = 0;
    while (true) {
      const data = Buffer.from(
        await this.rpc<string>(
          { op: "output", execution_id: executionId, offset, limit: 65536 },
          context.abortSignal,
        ),
        "base64",
      );
      if (!data.length) return;
      offset += data.length;
      yield data;
    }
  }
  async cleanup(_context: Context) {
    this.closing = true;
    const results = await Promise.all(this.executions);
    this.closed = true;
    await rm(this.directory, { recursive: true, force: true });
    if (results.some((result) => !result.ok && result.error.code === "unknown"))
      throw new Error(
        "Remote cleanup is unresolved; the allocation owner must reconcile or stop the sandbox",
      );
  }
}
