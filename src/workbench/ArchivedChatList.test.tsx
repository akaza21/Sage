import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../app/store";
import { ArchivedChatList } from "./ArchivedChatList";

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
    archivedChatError: {
      projectId: "project-a",
      chatId: "chat:old",
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

  const alert = screen.getByRole("alert");
  expect(alert).toHaveTextContent(
    "Stop this Chat's running turn before deleting it",
  );
  expect(alert).not.toHaveTextContent(/invalid request/i);
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

  fireEvent.click(
    screen.getByRole("button", { name: "Dismiss error for Research" }),
  );
  expect(screen.queryByRole("alert")).toBeNull();
});
