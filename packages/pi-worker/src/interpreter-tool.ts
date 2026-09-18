import { randomUUID } from "node:crypto";
import { Type } from "@earendil-works/pi-ai";
import type {
  AgentHarnessTool,
  ExecutionToolContext,
} from "@earendil-works/pi-agent-core";
import type { InterpreterClient } from "./interpreter-client.js";

const parameters = Type.Object({
  action: Type.Union(
    ["start", "execute", "checkpoint", "restore", "receipt", "stop"].map(
      (value) => Type.Literal(value),
    ),
  ),
  code: Type.Optional(Type.String()),
  names: Type.Optional(Type.Array(Type.String())),
  artifact_id: Type.Optional(Type.String()),
  request_id: Type.Optional(Type.String()),
});

export function createInterpreterTool(
  client: InterpreterClient,
): AgentHarnessTool<ExecutionToolContext, typeof parameters> {
  return {
    name: "python",
    label: "Python interpreter",
    description:
      "Run bounded Python in the assigned sandbox. Start explicitly. Variables last only while that interpreter lives. Checkpoint selected JSON objects to a retained artifact; restore explicitly after loss. An unknown execution requires receipt inspection and host reconciliation, never repetition.",
    parameters,
    replay: "never",
    async execute(_id, args, _update, _toolContext, invocation, context) {
      let response: unknown;
      let requestId: string | undefined;
      switch (args.action) {
        case "start":
          response = await client.start(context);
          break;
        case "execute": {
          if (args.code === undefined)
            throw new Error("Python execution requires code");
          requestId = (await invocation.getMemo("interpreter-request")) as
            string | undefined;
          if (!requestId) {
            requestId = randomUUID();
            await invocation.setMemo("interpreter-request", requestId);
          }
          response = await client.execute(requestId, args.code, context);
          break;
        }
        case "checkpoint": {
          if (!args.names?.length)
            throw new Error("Select checkpoint object names");
          let artifactId = (await invocation.getMemo(
            "interpreter-checkpoint",
          )) as string | undefined;
          if (!artifactId) {
            artifactId = randomUUID();
            await invocation.setMemo("interpreter-checkpoint", artifactId);
          }
          response = {
            artifact_id: await client.checkpoint(
              args.names,
              artifactId,
              context,
            ),
          };
          break;
        }
        case "restore":
          if (!args.artifact_id)
            throw new Error("Restore requires a checkpoint artifact");
          response = await client.restore(args.artifact_id, context);
          break;
        case "receipt":
          if (!args.request_id)
            throw new Error("Receipt lookup requires an execution request ID");
          response = await client.receipt(args.request_id, context);
          break;
        case "stop":
          response = await client.stop(context);
          break;
        default:
          throw new Error("Unsupported interpreter action");
      }
      return {
        content: [
          {
            type: "text",
            text: JSON.stringify({ request_id: requestId, response }),
          },
        ],
        details: response,
      };
    },
  };
}
