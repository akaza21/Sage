// A deterministic provider exercises real Pi RPC without credentials or network.
import { createAssistantMessageEventStream } from "@earendil-works/pi-ai";

export default function approvalFixture(pi) {
  const scenario = process.env.SAGE_PI_FIXTURE_SCENARIO ?? "echo";
  const names =
    scenario === "collisions"
      ? ["read", "write", "edit", "bash"]
      : ["fixture_echo"];
  let arrivals = 0;
  let release;
  const together = new Promise((resolve) => {
    release = resolve;
  });
  pi.registerCommand("fixture-command", {
    description: "Extension discovery fixture",
    handler: async (_args, ctx) => ctx.ui.notify("Fixture command available"),
  });
  if (scenario === "reload")
    pi.registerCommand("fixture-reload", {
      description: "Reload extension resources",
      handler: async (_args, ctx) => ctx.reload(),
    });
  for (const name of names)
    pi.registerTool({
      name,
      label: "Fixture echo",
      description: "Return the supplied text after Sage approval",
      parameters: {
        type: "object",
        properties: {
          text: { type: "string" },
          destination: {
            type: "object",
            properties: { path: { type: "string" } },
          },
        },
        required: ["text"],
        additionalProperties: false,
      },
      async execute(_id, args) {
        if (scenario === "concurrent") {
          if (++arrivals === 2) release();
          await together;
        }
        return {
          content: [
            {
              type: "text",
              text: `Executed: ${args.text}${args.destination ? ` -> ${args.destination.path}` : ""}`,
            },
          ],
          details: {},
        };
      },
    });
  pi.registerTool({
    name: "fixture_parallel",
    label: "Concurrent nested calls",
    description: "Run nested tools concurrently",
    parameters: { type: "object", properties: {}, additionalProperties: false },
    async execute(_id, _args, signal, _update, ctx) {
      if (scenario === "abandoned") {
        const held = ctx.executeTool(
          "fixture_echo",
          { text: "nested-held" },
          { signal },
        );
        const controllers = Array.from(
          { length: 8 },
          () => new AbortController(),
        );
        const abandoned = controllers.map((controller, index) =>
          ctx.executeTool(
            "fixture_echo",
            { text: `abandoned-${index}` },
            { signal: controller.signal },
          ),
        );
        await new Promise((resolve) => setTimeout(resolve, 25));
        controllers.forEach((controller) => controller.abort());
        // The hook cannot see these per-call signals. Its bounded wait must
        // release all eight slots, even while the first withdrawal is held.
        await new Promise((resolve) => setTimeout(resolve, 60100));
        const started = Date.now();
        const nextCall = ctx.executeTool(
          "fixture_echo",
          { text: "nested-after-timeout" },
          { signal },
        );
        await new Promise((resolve) => setTimeout(resolve, 25));
        ctx.ui.notify("Abandoned waiter probe queued");
        const next = await nextCall;
        if (next.isError || Date.now() - started >= 60000)
          throw new Error(
            "Abandoned waiters blocked the next call beyond the approval timeout",
          );
        await Promise.all([held, ...abandoned]);
        return {
          content: [
            { type: "text", text: "Abandoned approval slots released." },
          ],
          details: {},
        };
      }
      const count =
        scenario === "overflow" ? 10 : scenario === "concurrent" ? 3 : 2;
      const outcomes = await Promise.all(
        Array.from({ length: count }, (_, index) =>
          ctx.executeTool(
            scenario === "concurrent" && index === 2 ? "write" : "fixture_echo",
            scenario === "concurrent" && index === 2
              ? { path: "nested.txt", content: "approved write" }
              : { text: `nested-${index}` },
            { signal },
          ),
        ),
      );
      return {
        content: [
          {
            type: "text",
            text: JSON.stringify(outcomes.map(({ isError }) => isError)),
          },
        ],
        details: {},
      };
    },
  });
  pi.registerProvider("sage-fixture", {
    api: "sage-fixture-api",
    apiKey: "fixture",
    baseUrl: "http://fixture.invalid",
    models: [
      {
        id: "approval",
        name: "Approval fixture",
        reasoning: false,
        input: ["text"],
        cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
        contextWindow: 32000,
        maxTokens: 1000,
      },
    ],
    streamSimple(model, context) {
      const stream = createAssistantMessageEventStream();
      queueMicrotask(() => {
        const last = context.messages.at(-1);
        const result = last?.role === "toolResult";
        const prompt = last?.content?.[0]?.text ?? "hello";
        const content = result
          ? [{ type: "text", text: "Fixture complete." }]
          : (scenario === "collisions"
              ? [prompt]
              : [
                  ["concurrent", "overflow", "cancel", "abandoned"].includes(
                    scenario,
                  ) && prompt !== "after-cancel"
                    ? "fixture_parallel"
                    : "fixture_echo",
                ]
            ).map((name, index) => ({
              type: "toolCall",
              id: `fixture-call-${index}`,
              name,
              arguments:
                name === "fixture_parallel"
                  ? {}
                  : {
                      text: prompt,
                      ...(scenario === "mutation"
                        ? { destination: { path: "approved" } }
                        : {}),
                    },
            }));
        const message = {
          role: "assistant",
          api: model.api,
          provider: model.provider,
          model: model.id,
          content,
          stopReason: result ? "stop" : "toolUse",
          timestamp: Date.now(),
          usage: {
            input: 1,
            output: 1,
            cacheRead: 0,
            cacheWrite: 0,
            totalTokens: 2,
            cost: {
              input: 0,
              output: 0,
              cacheRead: 0,
              cacheWrite: 0,
              total: 0,
            },
          },
        };
        stream.push({ type: "start", partial: message });
        stream.push({ type: "done", reason: message.stopReason, message });
        stream.end();
      });
      return stream;
    },
  });
}
