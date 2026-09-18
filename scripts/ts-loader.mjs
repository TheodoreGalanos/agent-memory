// ABOUTME: Node module hooks that run this repository's TypeScript sources directly.
// ABOUTME: Resolves relative ".js" imports to ".ts" files and transpiles them without type checking.
import { existsSync, readFileSync } from "node:fs";
import { registerHooks } from "node:module";
import { fileURLToPath } from "node:url";
import ts from "typescript";

registerHooks({
  resolve(specifier, context, next) {
    const relative = specifier.startsWith("./") || specifier.startsWith("../");
    if (relative && specifier.endsWith(".js") && context.parentURL?.startsWith("file:")) {
      const javascript = new URL(specifier, context.parentURL);
      const typescript = new URL(`${specifier.slice(0, -3)}.ts`, context.parentURL);
      if (!existsSync(fileURLToPath(javascript)) && existsSync(fileURLToPath(typescript)))
        return next(typescript.href, context);
    }
    return next(specifier, context);
  },
  load(url, context, next) {
    if (!url.startsWith("file:") || !url.endsWith(".ts")) return next(url, context);
    const { outputText } = ts.transpileModule(readFileSync(fileURLToPath(url), "utf8"), {
      fileName: fileURLToPath(url),
      compilerOptions: {
        module: ts.ModuleKind.ESNext,
        target: ts.ScriptTarget.ES2022,
        verbatimModuleSyntax: true,
      },
    });
    return { format: "module", source: outputText, shortCircuit: true };
  },
});
