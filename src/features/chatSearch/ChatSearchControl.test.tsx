import {
  act,
  cleanup,
  createEvent,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useWorkbenchStore } from "../../app/store";
import type { ProjectChatSearchResponse } from "./chatSearchDomain";
import { projectChatSearchDesktop } from "./chatSearchDesktop";
import { ChatSearchControl } from "./ChatSearchControl";
import {
  clearChatSearchFolderRequest,
  requestChatSearchInFolder,
} from "./chatSearchScope";

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
    clearChatSearchFolderRequest();
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

  afterEach(() => clearChatSearchFolderRequest());

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
    expect(
      within(
        await screen.findByRole("listbox", { name: "Chat search results" }),
      ).getAllByRole("option"),
    ).toHaveLength(2);
    expect(search).toHaveBeenCalledWith({
      projectId: "project-a",
      projectGeneration: 3,
      expectedTransitionEpoch: 2,
      query: "needle",
    });
    fireEvent.change(input, { target: { value: "changed" } });
    expect(
      screen.queryByRole("listbox", { name: "Chat search results" }),
    ).toBeNull();
    fireEvent.change(input, { target: { value: "needle" } });
    fireEvent.submit(input.closest("form")!);
    await screen.findByRole("listbox", { name: "Chat search results" });
    fireEvent.click(screen.getByRole("option", { name: /Chat: B.*Attached/ }));
    expect(activateChat).toHaveBeenCalledWith("chat-b");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    fireEvent.click(trigger);
    const reopenedInput = screen.getByRole("searchbox", {
      name: "Search Chats",
    });
    fireEvent.change(reopenedInput, { target: { value: "needle" } });
    fireEvent.submit(reopenedInput.closest("form")!);
    await screen.findByRole("listbox", { name: "Chat search results" });
    fireEvent.click(
      screen.getByRole("option", { name: /Chat: Old.*Archived/ }),
    );
    expect(restoreChat).toHaveBeenCalledWith("chat-old", "project-a");
    expect(dialog).not.toBeInTheDocument();
  });

  it("consumes a close(false) focus return on activation and uses only a new return after reopen", async () => {
    vi.spyOn(projectChatSearchDesktop, "search").mockResolvedValue(response);
    const activated = document.createElement("button");
    activated.textContent = "Activated chat";
    document.body.append(activated);
    const activateChat = vi.fn(async () => activated.focus());
    useWorkbenchStore.setState({ activateChat });
    render(<ChatSearchControl />);

    const priorReturn = vi.fn();
    act(() => requestChatSearchInFolder("project-a", priorReturn));
    const firstInput = await screen.findByRole("searchbox", {
      name: "Search Chats",
    });
    fireEvent.change(firstInput, { target: { value: "needle" } });
    fireEvent.submit(firstInput.closest("form")!);
    await screen.findByRole("listbox", { name: "Chat search results" });
    fireEvent.click(screen.getByRole("option", { name: /Chat: B.*Attached/ }));
    await waitFor(() => expect(activateChat).toHaveBeenCalledWith("chat-b"));
    expect(priorReturn).not.toHaveBeenCalled();
    expect(activated).toHaveFocus();

    const nextReturn = vi.fn();
    act(() => requestChatSearchInFolder("project-a", nextReturn));
    await screen.findByRole("dialog", { name: "Search Chats" });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(nextReturn).toHaveBeenCalledOnce());
    expect(priorReturn).not.toHaveBeenCalled();
    activated.remove();
  });

  it("drops a rejected external focus return before a plain search is cancelled", async () => {
    render(<ChatSearchControl />);

    const staleReturn = vi.fn();
    act(() => {
      useWorkbenchStore.setState({ projectTransitioning: true });
      requestChatSearchInFolder("project-a", staleReturn);
    });
    expect(screen.queryByRole("dialog", { name: "Search Chats" })).toBeNull();

    act(() => useWorkbenchStore.setState({ projectTransitioning: false }));
    const trigger = screen.getByRole("button", { name: "Search chats" });
    fireEvent.click(trigger);
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    await waitFor(() => expect(trigger).toHaveFocus());
    expect(staleReturn).not.toHaveBeenCalled();
  });

  it("searches and restores an archived chat on another folder without switching", async () => {
    const foreignResponse: ProjectChatSearchResponse = {
      projectId: "cross",
      projectGeneration: 3,
      transitionEpoch: 2,
      query: "Research",
      results: [
        {
          chatId: "chat-old",
          title: "Chat: Research",
          attached: false,
          matchedTitle: true,
          snippet: null,
        },
      ],
      hasMore: false,
      elapsedMs: 2,
    };
    const search = vi
      .spyOn(projectChatSearchDesktop, "search")
      .mockResolvedValue(foreignResponse);
    const restoreChat = vi.fn().mockResolvedValue(true);
    const switchProject = vi.fn().mockResolvedValue(undefined);
    useWorkbenchStore.setState({
      projects: [
        { id: "project-a", name: "Sage", rootCount: 1, lastOpenedAt: 1 },
        {
          id: "cross",
          name: "Cross",
          rootCount: 1,
          lastOpenedAt: 2,
          railChats: [
            {
              id: "chat:seeded",
              title: "Chat: New project",
              pinned: true,
              position: 0,
            },
          ],
          railArchived: [{ id: "chat-old", title: "Chat: Research" }],
        },
      ],
      restoreChat,
      switchProject,
    });
    render(<ChatSearchControl />);
    fireEvent.click(screen.getByRole("button", { name: "Search chats" }));
    fireEvent.click(screen.getByRole("button", { name: "Search in" }));
    fireEvent.click(screen.getByRole("option", { name: "Cross" }));
    const input = screen.getByRole("searchbox", { name: "Search Chats" });
    fireEvent.change(input, { target: { value: "Research" } });
    fireEvent.submit(input.closest("form")!);
    expect(
      await screen.findByRole("option", { name: /Chat: Research/ }),
    ).toBeInTheDocument();
    expect(search).toHaveBeenCalledWith({
      projectId: "cross",
      projectGeneration: 3,
      expectedTransitionEpoch: 2,
      query: "Research",
    });
    fireEvent.click(
      screen.getByRole("option", { name: /Chat: Research.*Archived/ }),
    );
    expect(restoreChat).toHaveBeenCalledWith("chat-old", "cross");
    expect(switchProject).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState().project?.id).toBe("project-a");
  });

  it("fans out All folders and restores without switching", async () => {
    const search = vi
      .spyOn(projectChatSearchDesktop, "search")
      .mockImplementation(async (request) => {
        if (request.projectId === "cross") {
          return {
            projectId: "cross",
            projectGeneration: 3,
            transitionEpoch: 2,
            query: "Research",
            results: [
              {
                chatId: "chat-old",
                title: "Chat: Research",
                attached: false,
                matchedTitle: true,
                snippet: null,
              },
            ],
            hasMore: false,
            elapsedMs: 2,
          };
        }
        return {
          projectId: request.projectId,
          projectGeneration: 3,
          transitionEpoch: 2,
          query: "Research",
          results: [],
          hasMore: false,
          elapsedMs: 1,
        };
      });
    const restoreChat = vi.fn().mockResolvedValue(true);
    const switchProject = vi.fn().mockResolvedValue(undefined);
    useWorkbenchStore.setState({
      projects: [
        { id: "project-a", name: "Sage", rootCount: 1, lastOpenedAt: 1 },
        {
          id: "cross",
          name: "Cross",
          rootCount: 1,
          lastOpenedAt: 2,
          railArchived: [{ id: "chat-old", title: "Chat: Research" }],
        },
      ],
      restoreChat,
      switchProject,
    });
    render(<ChatSearchControl />);
    fireEvent.click(screen.getByRole("button", { name: "Search chats" }));
    fireEvent.click(screen.getByRole("button", { name: "Search in" }));
    fireEvent.click(screen.getByRole("option", { name: "All folders" }));
    const input = screen.getByRole("searchbox", { name: "Search Chats" });
    fireEvent.change(input, { target: { value: "Research" } });
    fireEvent.submit(input.closest("form")!);
    expect(
      await screen.findByRole("option", { name: /Chat: Research/ }),
    ).toHaveTextContent("Cross");
    expect(search.mock.calls.map((call) => call[0]?.projectId)).toEqual([
      "project-a",
      "cross",
    ]);
    fireEvent.click(
      screen.getByRole("option", { name: /Chat: Research.*Archived/ }),
    );
    expect(restoreChat).toHaveBeenCalledWith("chat-old", "cross");
    expect(switchProject).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState().project?.id).toBe("project-a");
  });

  it("closes an actually open scope picker when its owner becomes stale", async () => {
    render(<ChatSearchControl />);
    const trigger = screen.getByRole("button", { name: "Search chats" });
    fireEvent.click(trigger);
    fireEvent.click(screen.getByRole("button", { name: "Search in" }));
    const input = screen.getByRole("combobox", { name: "Search in" });
    await waitFor(() => expect(input).toHaveFocus());
    expect(screen.getByRole("listbox", { name: "Search in" })).toBeVisible();
    act(() => useWorkbenchStore.setState({ projectGeneration: 4 }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(screen.queryByRole("combobox", { name: "Search in" })).toBeNull();
    await waitFor(() => expect(trigger).toHaveFocus());
  });

  it("drops a stale completion after a Project intent changes", async () => {
    const pending = deferred<typeof response>();
    vi.spyOn(projectChatSearchDesktop, "search").mockReturnValue(
      pending.promise,
    );
    render(<ChatSearchControl />);
    fireEvent.click(screen.getByRole("button", { name: "Search chats" }));
    fireEvent.click(screen.getByRole("button", { name: "Search in" }));
    expect(screen.getByRole("combobox", { name: "Search in" })).toBeVisible();
    fireEvent.keyDown(screen.getByRole("combobox", { name: "Search in" }), {
      key: "Escape",
    });
    fireEvent.click(screen.getByRole("button", { name: "Search in" }));
    const input = screen.getByRole("searchbox", { name: "Search Chats" });
    fireEvent.change(input, { target: { value: "needle" } });
    fireEvent.submit(input.closest("form")!);
    useWorkbenchStore.setState({ projectGeneration: 4 });
    pending.resolve(response);

    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
    expect(screen.queryByRole("combobox", { name: "Search in" })).toBeNull();
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
    await screen.findByRole("listbox", { name: "Chat search results" });
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

  it("keeps keyboard focus inside the named chat-search dialog", async () => {
    useWorkbenchStore.setState({
      projects: Array.from({ length: 64 }, (_, index) => ({
        id: index === 0 ? "project-a" : `project-${index + 1}`,
        name: index === 0 ? "A" : `Project ${index + 1}`,
        rootCount: 1,
        lastOpenedAt: index + 1,
      })),
    });
    render(<ChatSearchControl />);
    fireEvent.click(screen.getByRole("button", { name: "Search chats" }));
    const dialog = screen.getByRole("dialog", { name: "Search Chats" });
    const select = screen.getByRole("button", { name: "Search in" });
    const search = screen.getByRole("button", { name: "Search" });

    fireEvent.click(select);
    const pickerInput = screen.getByRole("combobox", { name: "Search in" });
    expect(dialog.contains(pickerInput)).toBe(true);
    await waitFor(() => expect(pickerInput).toHaveFocus());
    expect(screen.getAllByRole("option")).toHaveLength(65);
    const forwardTab = createEvent.keyDown(pickerInput, { key: "Tab" });
    fireEvent(pickerInput, forwardTab);
    expect(forwardTab.defaultPrevented).toBe(false);
    expect(screen.queryByRole("combobox", { name: "Search in" })).toBeNull();
    expect(select).toHaveFocus();
    fireEvent.click(select);
    const reverseInput = screen.getByRole("combobox", { name: "Search in" });
    await waitFor(() => expect(reverseInput).toHaveFocus());
    const reverseTab = createEvent.keyDown(reverseInput, {
      key: "Tab",
      shiftKey: true,
    });
    fireEvent(reverseInput, reverseTab);
    expect(reverseTab.defaultPrevented).toBe(true);
    expect(screen.queryByRole("combobox", { name: "Search in" })).toBeNull();
    fireEvent.click(select);
    const escapeInput = screen.getByRole("combobox", { name: "Search in" });
    await waitFor(() => expect(escapeInput).toHaveFocus());
    fireEvent.keyDown(escapeInput, { key: "Escape" });
    expect(screen.queryByRole("combobox", { name: "Search in" })).toBeNull();
    expect(dialog).toBeVisible();
    expect(select).toHaveFocus();

    search.focus();
    fireEvent.keyDown(dialog, { key: "Tab" });
    expect(select).toHaveFocus();

    select.focus();
    fireEvent.keyDown(dialog, { key: "Tab", shiftKey: true });
    expect(search).toHaveFocus();
  });
});
