import {
  createAssistantMessageEventStream,
  type AssistantMessage,
  type Api,
  type Model,
  type Models,
  type ModelsSimpleStreamOptions,
  type ModelsDeferredFetchOptions,
} from "@earendil-works/pi-ai";

/** Pi reports most hook errors and continues. Enforce application admission
 * failures outside hook aggregation, including after the final payload hook. */
export function guardProviderCalls(models: Models) {
  let failure: Error | undefined;
  function check() {
    if (failure) throw failure;
  }
  function blocked(model: Model<Api>) {
    const stream = createAssistantMessageEventStream();
    const message: AssistantMessage = {
      role: "assistant",
      content: [],
      api: model.api,
      model: model.id,
      provider: model.provider,
      stopReason: "error",
      errorMessage: failure!.message,
      timestamp: Date.now(),
      usage: {
        input: 0,
        output: 0,
        cacheRead: 0,
        cacheWrite: 0,
        totalTokens: 0,
        cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
      },
    };
    stream.push({ type: "error", reason: "error", error: message });
    stream.end(message);
    return stream;
  }
  function guarded<
    T extends ModelsSimpleStreamOptions | ModelsDeferredFetchOptions,
  >(options: T | undefined): T {
    return {
      ...options,
      async onPayload(payload, model) {
        const result = await options?.onPayload?.(payload, model);
        check();
        return result;
      },
    } as T;
  }
  const protectedModels = new Proxy(models, {
    get(target, key) {
      if (key === "streamSimple")
        return ((model, transcript, options) => {
          if (failure) return blocked(model);
          return target.streamSimple(model, transcript, guarded(options));
        }) satisfies Models["streamSimple"];
      if (key === "streamDeferred")
        return ((model, handle, options) => {
          if (failure) return blocked(model);
          return target.streamDeferred(model, handle, guarded(options));
        }) satisfies Models["streamDeferred"];
      const member = Reflect.get(target, key, target);
      return typeof member === "function" ? member.bind(target) : member;
    },
  });
  return {
    models: protectedModels,
    fail(message: string) {
      failure ??= new Error(message);
    },
  };
}
