import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import type { ProjectChatSearchResponse } from "./chatSearchDomain";
import { projectChatSearchDesktop } from "./chatSearchDesktop";
import { ChatSearchControl } from "./ChatSearchControl";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

const response: ProjectChatSearchResponse = {
  projectId: "project-a",
  projectGeneration: 3,
  transitionEpoch: 2,
  query: "needle",
  results: [
    {
      chatId: "chat-b",
      title: "Chat: B",
      attached: true,
      matchedTitle: false,
      snippet: "attached needle context",
    },
    {
      chatId: "chat-old",
      title: "Chat: Old",
      attached: false,
      matchedTitle: true,
      snippet: null,
    },
  ],
  hasMore: false,
  elapsedMs: 2,
};

describe("ChatSearchControl", () => {
  afterEach(cleanup);

  beforeEach(() => {
    useWorkbenchStore.setState(useWorkbenchStore.getInitialState(), true);
    useWorkbenchStore.setState({
      phase: "ready",
      projectGeneration: 3,
      nativeTransitionEpoch: 2,
      projectTransitioning: false,
      projects: [{ id: "project-a", name: "A", rootCount: 1, lastOpenedAt: 1 }],
      project: {
        id: "project-a",
        name: "A",
        roots: [
          {
            id: "root-a",
            name: "A",
            ordinal: 0,
            availability: "available",
            message: null,
          },
        ],
      },
      tabs: [
        {
          id: "chat-a",
          kind: "chat",
          title: "Chat: A",
          position: 0,
          pinned: true,
          dirty: false,
        },
        {
          id: "chat-b",
          kind: "chat",
          title: "Chat: B",
          position: 1,
          pinned: false,
          dirty: false,
        },
      ],
      archivedChats: [{ id: "chat-old", title: "Chat: Old" }],
      activeTabId: "chat-a",
      selectedRootId: "root-a",
    });
    vi.restoreAllMocks();
  });

  it("submits explicitly, edits criteria, and navigates attached and archived results", async () => {
    const search = vi
      .spyOn(projectChatSearchDesktop, "search")
      .mockResolvedValue(response);
    const activateChat = vi.fn().mockResolvedValue(undefined);
    const restoreChat = vi.fn().mockResolvedValue(true);
    useWorkbenchStore.setState({ activateChat, restoreChat });
    render(<ChatSearchControl />);

    const trigger = screen.getByRole("button", { name: "Search chats" });
    fireEvent.click(trigger);
    const dialog = screen.getByRole("dialog", { name: "Search Chats" });
    const input = screen.getByRole("searchbox", { name: "Search Chats" });
    expect(input).toHaveFocus();
    fireEvent.submit(input.closest("form")!);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Enter text to search for.",
    );

    fireEvent.change(input, { target: { value: "needle" } });
    expect(search).not.toHaveBeenCalled();
    fireEvent.submit(input.closest("form")!);
    expect(await screen.findAllByRole("option")).toHaveLength(2);
    expect(search).toHaveBeenCalledWith({
      projectId: "project-a",
      projectGeneration: 3,
      expectedTransitionEpoch: 2,
      query: "needle",
    });
    fireEvent.change(input, { target: { value: "changed" } });
    expect(screen.queryAllByRole("option")).toHaveLength(0);
    fireEvent.change(input, { target: { value: "needle" } });
    fireEvent.submit(input.closest("form")!);
    await screen.findAllByRole("option");
    fireEvent.click(screen.getByRole("option", { name: /Chat: B.*Attached/ }));
    expect(activateChat).toHaveBeenCalledWith("chat-b");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    fireEvent.click(trigger);
    const reopenedInput = screen.getByRole("searchbox", {
      name: "Search Chats",
    });
    fireEvent.change(reopenedInput, { target: { value: "needle" } });
    fireEvent.submit(reopenedInput.closest("form")!);
    await screen.findAllByRole("option");
    fireEvent.click(
      screen.getByRole("option", { name: /Chat: Old.*Archived/ }),
    );
    expect(restoreChat).toHaveBeenCalledWith("chat-old");
    expect(dialog).not.toBeInTheDocument();
  });

  it("drops a stale completion after a Project intent changes", async () => {
    const pending = deferred<typeof response>();
    vi.spyOn(projectChatSearchDesktop, "search").mockReturnValue(
      pending.promise,
    );
    render(<ChatSearchControl />);
    fireEvent.click(screen.getByRole("button", { name: "Search chats" }));
    const input = screen.getByRole("searchbox", { name: "Search Chats" });
    fireEvent.change(input, { target: { value: "needle" } });
    fireEvent.submit(input.closest("form")!);
    useWorkbenchStore.setState({ projectGeneration: 4 });
    pending.resolve(response);

    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(
      screen.queryByText("attached needle context"),
    ).not.toBeInTheDocument();
  });

  it("cancels a pending disposable search without applying its completion", async () => {
    const pending = deferred<typeof response>();
    vi.spyOn(projectChatSearchDesktop, "search").mockReturnValue(
      pending.promise,
    );
    render(<ChatSearchControl />);
    const trigger = screen.getByRole("button", { name: "Search chats" });
    fireEvent.click(trigger);
    const input = screen.getByRole("searchbox", { name: "Search Chats" });
    fireEvent.change(input, { target: { value: "needle" } });
    fireEvent.submit(input.closest("form")!);
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    pending.resolve(response);

    await waitFor(() => expect(trigger).toHaveFocus());
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(
      screen.queryByText("attached needle context"),
    ).not.toBeInTheDocument();
  });

  it("renders an empty state and refuses a result whose attachment changed", async () => {
    const search = vi.spyOn(projectChatSearchDesktop, "search");
    search.mockResolvedValueOnce({ ...response, results: [] });
    search.mockResolvedValueOnce(response);
    const restoreChat = vi.fn().mockResolvedValue(true);
    useWorkbenchStore.setState({ restoreChat });
    render(<ChatSearchControl />);
    fireEvent.click(screen.getByRole("button", { name: "Search chats" }));
    const input = screen.getByRole("searchbox", { name: "Search Chats" });
    fireEvent.change(input, { target: { value: "needle" } });
    fireEvent.submit(input.closest("form")!);
    expect(await screen.findByText(/No Chats matched/)).toBeVisible();

    fireEvent.submit(input.closest("form")!);
    await screen.findAllByRole("option");
    useWorkbenchStore.setState({
      archivedChats: [],
      tabs: [
        ...useWorkbenchStore.getState().tabs,
        {
          id: "chat-old",
          kind: "chat",
          title: "Chat: Old",
          position: 2,
          pinned: false,
          dirty: false,
        },
      ],
    });
    fireEvent.click(
      screen.getByRole("option", { name: /Chat: Old.*Archived/ }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "result changed",
    );
    expect(restoreChat).not.toHaveBeenCalled();
  });

  it("shows bounded failures and returns focus on cancel or Escape", async () => {
    vi.spyOn(projectChatSearchDesktop, "search").mockRejectedValue(
      new Error("browser unavailable"),
    );
    render(<ChatSearchControl />);
    const trigger = screen.getByRole("button", { name: "Search chats" });
    fireEvent.click(trigger);
    const dialog = screen.getByRole("dialog", { name: "Search Chats" });
    const input = screen.getByRole("searchbox", { name: "Search Chats" });
    fireEvent.change(input, { target: { value: "needle" } });
    fireEvent.submit(input.closest("form")!);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "browser unavailable",
    );
    fireEvent.keyDown(dialog, { key: "Escape" });
    await waitFor(() => expect(trigger).toHaveFocus());
  });
});
