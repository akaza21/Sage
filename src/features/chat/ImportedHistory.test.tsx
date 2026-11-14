import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

const { invokeNative } = vi.hoisted(() => ({ invokeNative: vi.fn() }));
vi.mock("../../ports/nativeInvoke", async (original) => ({
  ...(await original()),
  invokeNative,
}));

import { acpProjectionStoreFor } from "../../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage } from "../../domain/acp";
import { ImportedHistory } from "./ImportedHistory";

afterEach(cleanup);

it("renders the imported Turns as inherited Turns, an unfinished one stopped", async () => {
  useWorkbenchStore.setState({
    project: { id: "project-1" },
  } as unknown as Parameters<typeof useWorkbenchStore.setState>[0]);
  acpProjectionStoreFor("chat-imported").setState({
    page: {
      projectId: "project-1",
      chatId: "chat-imported",
      currentSessionId: "session-1",
      sessions: [],
      turns: [],
    } as unknown as AcpChatPage,
  });
  invokeNative.mockResolvedValueOnce({
    sessionId: "session-1",
    runtimeEpoch: "1",
    retentionLimited: false,
    turns: [
      {
        id: "turn-1",
        status: "inProgress",
        items: [
          {
            id: "prompt-1",
            role: "user",
            lifecycle: "completed",
            payload: { kind: "text", payload: { text: "Fix the build" } },
          },
          {
            id: "think-1",
            lifecycle: "started",
            payload: { kind: "thoughtSummary", payload: { summary: "Plan" } },
          },
        ],
      },
    ],
  });
  const { container } = render(<ImportedHistory chatId="chat-imported" />);
  expect(await screen.findByText("Fix the build")).toBeTruthy();
  expect(invokeNative).toHaveBeenCalledWith("read_imported_history", {
    projectId: "project-1",
    chatId: "chat-imported",
  });
  const turn = container.querySelector(".chat-fork-turn .acp-turn");
  expect(turn?.getAttribute("data-turn-state")).toBe("interrupted");
  expect(container.querySelector(".acp-live-shimmer")).toBeNull();
  expect(screen.getByText("Continued in Sage")).toBeTruthy();
});
