import { expect, it, vi } from "vitest";
import { createNativeWorkbenchPorts } from "./nativeWorkbenchPorts";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("./nativeInvoke", () => ({ invokeNative: invoke }));

it("routes the fork through the typed native Chat port with exact ownership (R3-12 budget cut)", async () => {
  const request = {
    currentLayout: {
      projectId: "project-1",
      tabs: [],
      activeTabId: "chat-1",
      selectedRootId: null,
      selectedResource: null,
    },
    chatId: "chat-1",
    turnId: "turn-1",
    expectedTransitionEpoch: 7,
  };
  const result = { transitionEpoch: 8 };
  invoke.mockResolvedValue(result);
  const fork = createNativeWorkbenchPorts().chat.forkChat;
  expect(fork).toBeTypeOf("function");
  await expect(fork!(request)).resolves.toBe(result);
  expect(invoke).toHaveBeenCalledWith("fork_chat", request);
});
