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
