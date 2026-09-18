import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

/** Trusted local lifecycle process. Its private configuration never enters Pi context. */
export class SandboxClient {
  private pending: Promise<unknown> = Promise.resolve();
  constructor(
    private readonly python: string,
    private readonly config: string,
  ) {}
  command<T>(request: Record<string, unknown>): Promise<T> {
    const call = this.pending.then(() => this.invoke<T>(request));
    this.pending = call.catch(() => undefined);
    return call;
  }
  private invoke<T>(request: Record<string, unknown>): Promise<T> {
    const script = fileURLToPath(
      new URL("../../../services/harbor-bridge/cli.py", import.meta.url),
    );
    return new Promise((resolve, reject) => {
      const child = spawn(this.python, [script, this.config], {
        stdio: ["pipe", "pipe", "pipe"],
        timeout: request.action === "create" ? 240_000 : 120_000,
      });
      const output: Buffer[] = [];
      let bytes = 0;
      let diagnostic = "";
      child.stdout.on("data", (part: Buffer) => {
        bytes += part.length;
        if (bytes > 2 * 1024 * 1024) child.kill();
        else output.push(part);
      });
      child.stderr.on("data", (part: Buffer) => {
        diagnostic = (diagnostic + part.toString("utf8")).slice(-4096);
      });
      child.stdin.on("error", () => {});
      child.once("error", reject);
      child.once("close", (code) => {
        if (code !== 0 || bytes > 2 * 1024 * 1024) {
          let reason = "";
          try {
            const detail = JSON.parse(
              diagnostic.trim().split("\n").at(-1)!,
            ) as { error: string; status?: number };
            if (/^[A-Za-z]+$/.test(detail.error))
              reason = ` (${detail.error}${Number.isInteger(detail.status) ? ` ${detail.status}` : ""})`;
          } catch {}
          return reject(
            new Error(
              `Sandbox lifecycle command failed${reason}; retained state requires reconciliation`,
            ),
          );
        }
        try {
          resolve(JSON.parse(Buffer.concat(output).toString("utf8")) as T);
        } catch {
          reject(new Error("Sandbox lifecycle returned an invalid response"));
        }
      });
      child.stdin.end(JSON.stringify(request));
    });
  }
}
