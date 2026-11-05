import { cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, expect, it } from "vitest";

import { useWorkbenchStore } from "../app/store";
import {
  chatReadThrough,
  resetChatReadMarks,
} from "../features/chat/acp/chatReadMarks";
import type { WorkbenchTab } from "../domain/workbench";
import { ViewedChatReadMark } from "./ViewedChatReadMark";

const initial = useWorkbenchStore.getState();

function finishedChat(overrides: Partial<WorkbenchTab> = {}): WorkbenchTab {
  return {
    id: "chat-a",
    kind: "chat",
    title: "Finished",
    position: 0,
    pinned: false,
    dirty: false,
    status: "complete",
    updatedAt: 9_000,
    finishedAt: 5_000,
    ...overrides,
  };
}

beforeEach(() => {
  resetChatReadMarks();
  useWorkbenchStore.setState({
    tabs: [finishedChat()],
    activeTabId: "chat-a",
    projectTransitioning: false,
  });
});

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initial, true);
  resetChatReadMarks();
});

it("records the visible chat as read without the sidebar mounted", () => {
  render(<ViewedChatReadMark visible />);
  // Through the latest Turn's finish time, not the chat's update time.
  expect(chatReadThrough("chat-a")).toBe(5_000);
});

it("records nothing while the chat is hidden or still running", () => {
  const { rerender } = render(<ViewedChatReadMark visible={false} />);
  expect(chatReadThrough("chat-a")).toBeUndefined();
  useWorkbenchStore.setState({ tabs: [finishedChat({ status: "working" })] });
  rerender(<ViewedChatReadMark visible />);
  expect(chatReadThrough("chat-a")).toBeUndefined();
});
