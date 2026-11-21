// A deterministic provider exercises real Pi RPC without credentials or network.
import { createAssistantMessageEventStream } from "@earendil-works/pi-ai";

export default function approvalFixture(pi) {
  pi.registerCommand("fixture-command", {
    description: "Extension discovery fixture",
    handler: async (_args, ctx) => ctx.ui.notify("Fixture command available"),
  });
  pi.registerTool({
    name: "fixture_echo",
    label: "Fixture echo",
    description: "Return the supplied text after Sage approval",
    parameters: {
      type: "object",
      properties: { text: { type: "string" } },
      required: ["text"],
      additionalProperties: false,
    },
    async execute(_id, args) {
      return {
        content: [{ type: "text", text: `Executed: ${args.text}` }],
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
          : [
              {
                type: "toolCall",
                id: "fixture-call",
                name: "fixture_echo",
                arguments: { text: prompt },
              },
            ];
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
