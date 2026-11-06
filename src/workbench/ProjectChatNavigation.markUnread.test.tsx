import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import {
  chatReadThrough,
  resetChatReadMarks,
  restoreChatReadMarks,
} from "../features/chat/acp/chatReadMarks";
import { ProjectChatNavigation } from "./ProjectChatNavigation";
import { clearAgentRailUnread } from "./agentRailInteractions";

const FINISHED_AT = 5_000;

// The settings store holds Beta's read mark and refuses to delete it.
vi.mock("../ports/nativeInvoke", async (importOriginal) => ({
  ...((await importOriginal()) as object),
  invokeNative: async (command: string) => {
    if (command === "chat_read_marks")
      return [{ chatId: "chat:b", readThrough: FINISHED_AT }];
    if (command === "chat_mark_unread")
      throw new Error("settings store is busy");
    return null;
  },
}));

const initialState = useWorkbenchStore.getState();

beforeEach(async () => {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  resetChatReadMarks();
  await restoreChatReadMarks();
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "A", roots: [] },
    tabs: [
      {
        id: "chat:a",
        kind: "chat",
        title: "Alpha",
        position: 0,
        pinned: false,
        dirty: false,
        status: "idle",
      },
      {
        id: "chat:b",
        kind: "chat",
        title: "Beta",
        position: 1,
        pinned: false,
        dirty: false,
        status: "complete",
        finishedAt: FINISHED_AT,
      },
    ],
    activeTabId: "chat:a",
  });
});

afterEach(() => {
  cleanup();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  resetChatReadMarks();
  clearAgentRailUnread();
  useWorkbenchStore.setState(initialState, true);
});

const betaUnread = () =>
  screen
    .getByRole("button", { name: /^Open chat Beta(?:,|$)/ })
    .closest(".project-chat-row")
    ?.querySelector("[aria-label='Unread updates']") ?? null;

it("shows the chat read again when the store refuses Mark unread", async () => {
  render(
    <ProjectChatNavigation
      projectId="project-a"
      chats={useWorkbenchStore.getState().tabs}
      selected
    />,
  );
  expect(betaUnread()).toBeNull();
  fireEvent.click(
    screen.getByRole("button", { name: "Chat actions for Beta" }),
  );
  fireEvent.click(await screen.findByRole("menuitem", { name: "Mark unread" }));
  // The store still holds the mark, so the row must not stay Unread.
  await vi.waitFor(() => expect(chatReadThrough("chat:b")).toBe(FINISHED_AT));
  await vi.waitFor(() => expect(betaUnread()).toBeNull());
});
