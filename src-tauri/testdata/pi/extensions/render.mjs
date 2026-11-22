import { createAssistantMessageEventStream } from "@earendil-works/pi-ai";

export default function renderFixture(pi) {
  let inject = false;
  pi.on("before_agent_start", async (event, ctx) => {
    // setModel alone emits no event; Sage reads it from the reply's model.
    if (event.prompt === "model-only") {
      await pi.setModel(ctx.modelRegistry.find("sage-render", "physical"));
      return;
    }
    if (event.prompt !== "render") return;
    inject = true;
    await prepare(ctx);
  });
  pi.on("agent_start", () => {
    if (!inject) return;
    inject = false;
    pi.sendUserMessage("Injected request", { deliverAs: "followUp" });
  });
  pi.registerCommand("fixture-render", {
    description: "Extension rendering fixture",
    handler: async (_args, ctx) => prepare(ctx),
  });
  async function prepare(ctx) {
    pi.sendMessage({
      customType: "fixture-report",
      content: "<script>safe text</script>",
      display: true,
      details: { note: "x".repeat(20000) },
    });
    pi.sendMessage({
      customType: "fixture-hidden",
      content: "Hidden message",
      display: false,
    });
    await pi.setModel(ctx.modelRegistry.find("sage-router", "virtual"));
    pi.setThinkingLevel("low");
  }
  pi.registerCommand("fixture-unknown", {
    description: "Unknown augmented message role",
    handler: () => pi.sendUserMessage("unknown-role"),
  });
  pi.on("session_start", () => {
    pi.registerProvider("sage-render", {
      api: "sage-render-api",
      apiKey: "fixture",
      baseUrl: "http://fixture.invalid",
      models: [
        {
          id: "physical",
          name: "Render fixture",
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
          const user = context.messages.findLast(
            (message) => message.role === "user",
          );
          const text =
            typeof user?.content === "string"
              ? user.content
              : user?.content?.[0]?.text;
          const message = {
            role: text === "unknown-role" ? "futureRole" : "assistant",
            api: model.api,
            provider: model.provider,
            model: model.id,
            content: [{ type: "text", text: "Fixture reply" }],
            stopReason: "stop",
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
          stream.push({ type: "done", reason: "stop", message });
          stream.end();
        });
        return stream;
      },
    });
    pi.registerVirtualModel({
      provider: "sage-router",
      id: "virtual",
      name: "Fixture virtual",
      thinkingLevels: ["off", "low", "high"],
      contextWindow: 32000,
      maxTokens: 1000,
      route(_request, ctx) {
        return { model: ctx.modelRegistry.find("sage-render", "physical") };
      },
    });
  });
}
