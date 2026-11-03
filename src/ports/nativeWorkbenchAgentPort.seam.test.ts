import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeNative = vi.fn();

vi.mock("./nativeInvoke", () => ({
  invokeNative: (command: string, args?: Record<string, unknown>) => {
    if (args === undefined) {
      return invokeNative(command);
    }
    return invokeNative(command, args);
  },
}));

describe("native agent port", () => {
  beforeEach(() => {
    invokeNative.mockReset();
  });

  it("forwards the harness catalog and prompt envelopes", async () => {
    const { createNativeWorkbenchPorts } =
      await import("./nativeWorkbenchPorts");
    const nativeWorkbenchAgentPort = createNativeWorkbenchPorts().agent;
    const request = {
      chatId: "chat-a",
      harnessId: "codex",
      modelId: "model-a",
      text: "Explain this change",
    };

    await nativeWorkbenchAgentPort.listHarnesses();
    await nativeWorkbenchAgentPort.listHarnessModels("codex");
    await nativeWorkbenchAgentPort.sendPrompt(request);

    expect(invokeNative).toHaveBeenNthCalledWith(1, "list_harnesses");
    expect(invokeNative).toHaveBeenNthCalledWith(2, "list_harness_models", {
      harnessId: "codex",
    });
    expect(invokeNative).toHaveBeenNthCalledWith(3, "send_simulated_prompt", {
      ...request,
    });
  });
});
