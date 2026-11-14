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

const store = vi.hoisted(() => ({
  marks: new Map<string, number>(),
  sequence: 0,
  refuseUnread: false,
}));
vi.mock("../ports/nativeInvoke", async (importOriginal) => ({
  ...((await importOriginal()) as object),
  invokeNative: async (
    command: string,
    args?: { chatId?: string; mark?: { chatId: string; readThrough: number } },
  ) => {
    if (command === "chat_read_marks")
      return {
        marks: [...store.marks].map(([chatId, readThrough]) => ({
          chatId,
          readThrough,
        })),
        capacity: 64,
      };
    if (command === "chat_mark_unread") {
      if (store.refuseUnread) throw new Error("settings store is busy");
      store.marks.delete(args!.chatId!);
      return null;
    }
    if (command === "chat_mark_read") {
      store.marks.set(args!.mark!.chatId, args!.mark!.readThrough);
      store.sequence += 1;
      return {
        ...args!.mark,
        evicted: [],
        sequence: store.sequence,
        capacity: 64,
      };
    }
    return null;
  },
}));

const initialState = useWorkbenchStore.getState();

beforeEach(async () => {
  Object.assign(window, { __TAURI_INTERNALS__: {} });
  store.marks = new Map([["chat:b", FINISHED_AT]]);
  store.refuseUnread = false;
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

function renderRail() {
  return render(
    <ProjectChatNavigation
      projectId="project-a"
      chats={useWorkbenchStore.getState().tabs}
      selected
    />,
  );
}

async function markBetaUnread() {
  fireEvent.click(
    screen.getByRole("button", { name: "Chat actions for Beta" }),
  );
  fireEvent.click(await screen.findByRole("menuitem", { name: "Mark unread" }));
}

it("keeps a chat marked unread Unread after a restart", async () => {
  renderRail();
  await markBetaUnread();
  await vi.waitFor(() => expect(store.marks.has("chat:b")).toBe(false));
  cleanup();
  clearAgentRailUnread();
  resetChatReadMarks();
  await restoreChatReadMarks();
  renderRail();
  expect(betaUnread()).not.toBeNull();
});

it("offers Mark unread only on a chat whose last Turn finished", async () => {
  renderRail();
  fireEvent.click(
    screen.getByRole("button", { name: "Chat actions for Alpha" }),
  );
  expect(
    await screen.findByRole("menu", { name: "Actions for Alpha" }),
  ).toBeInTheDocument();
  expect(screen.queryByRole("menuitem", { name: "Mark unread" })).toBeNull();
});

it("shows the chat read again when the store refuses Mark unread", async () => {
  store.refuseUnread = true;
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
  await vi.waitFor(() => expect(chatReadThrough("chat:b")).toBe(FINISHED_AT));
  await vi.waitFor(() => expect(betaUnread()).toBeNull());
});
