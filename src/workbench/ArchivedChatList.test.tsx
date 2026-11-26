import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import ArchivedChatList from "./ArchivedChatList";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => {
    throw new Error("refused");
  }),
}));

// Native runtime, so the cleanup goes to the mocked Tauri command.
Object.assign(window, { __TAURI_INTERNALS__: {} });
const initialState = useWorkbenchStore.getState();

afterEach(() => {
  cleanup();
  useWorkbenchStore.setState(initialState, true);
});

it("deletes an archived chat only after the irreversible delete is confirmed", () => {
  const deleteChat = vi.fn(async () => true);
  useWorkbenchStore.setState({ deleteChat });
  render(
    <ArchivedChatList
      projectId="project-a"
      chats={[{ id: "chat:old", title: "Chat: Research" }]}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: /^Archived chats:/ }));

  fireEvent.click(screen.getByRole("button", { name: "Delete chat Research" }));
  expect(screen.getByRole("dialog")).toHaveTextContent("Delete Research?");
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(deleteChat).not.toHaveBeenCalled();

  fireEvent.click(screen.getByRole("button", { name: "Delete chat Research" }));
  fireEvent.click(screen.getByRole("button", { name: "Delete chat" }));
  expect(deleteChat).toHaveBeenCalledOnce();
  expect(deleteChat).toHaveBeenCalledWith("chat:old", "project-a");
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("shows a refused action on the archived row it belongs to and dismisses it", () => {
  const restoreChat = vi.fn(async () => false);
  useWorkbenchStore.setState({
    restoreChat,
    chatRowError: {
      projectId: "project-a",
      chatId: "chat:old",
      action: "delete",
      message: "Stop this Chat's running turn before deleting it",
    },
  });
  render(
    <ArchivedChatList
      projectId="project-a"
      chats={[
        { id: "chat:old", title: "Chat: Research" },
        { id: "chat:other", title: "Chat: Notes" },
      ]}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: /^Archived chats:/ }));

  const notice = document.querySelector(".chat-row-error .notice--warning")!;
  // The refusal stays on its own row, not in a workbench-wide banner.
  expect(notice.closest(".archived-chat-row")).toHaveTextContent("Research");
  expect(notice).toHaveTextContent(
    "Stop this Chat's running turn before deleting it",
  );
  expect(notice).not.toHaveTextContent(/invalid request/i);
  expect(
    screen.getByRole("button", { name: "Delete chat Research" }),
  ).toHaveAccessibleDescription(
    "Stop this Chat's running turn before deleting it",
  );
  expect(
    screen.getByRole("button", { name: "Delete chat Notes" }),
  ).not.toHaveAttribute("aria-describedby");

  fireEvent.click(
    screen.getByRole("button", { name: "Restore chat Research" }),
  );
  expect(restoreChat).toHaveBeenCalledWith("chat:old", "project-a", "row");

  fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
  expect(document.querySelector(".chat-row-error")).toBeNull();
  expect(
    screen.getByRole("button", { name: "Delete chat Research" }),
  ).toHaveFocus();
});

it("returns focus to Restore after dismissing a refused restore", () => {
  useWorkbenchStore.setState({
    chatRowError: {
      projectId: "project-a",
      chatId: "chat:old",
      action: "restore",
      message: "Restore refused",
    },
  });
  render(
    <ArchivedChatList
      projectId="project-a"
      chats={[{ id: "chat:old", title: "Chat: Research" }]}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: /^Archived chats:/ }));
  fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
  expect(
    screen.getByRole("button", { name: "Restore chat Research" }),
  ).toHaveFocus();
});

it("deletes every archived chat of the project in one confirmed native cleanup", async () => {
  const { invoke } = await import("@tauri-apps/api/core");
  useWorkbenchStore.setState({ nativeTransitionEpoch: 3 });
  render(
    <ArchivedChatList
      projectId="project-a"
      chats={[
        { id: "chat:old", title: "Chat: Research" },
        { id: "chat:other", title: "Chat: Notes" },
      ]}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: /^Archived chats:/ }));
  fireEvent.click(screen.getByRole("button", { name: "Delete all archived" }));
  expect(screen.getByRole("dialog")).toHaveTextContent(
    "Delete all archived chats?",
  );
  fireEvent.click(screen.getByRole("button", { name: "Delete all" }));
  await waitFor(() =>
    expect(invoke).toHaveBeenCalledWith("delete_archived_chats", {
      projectId: "project-a",
      expectedTransitionEpoch: 3,
    }),
  );
});
