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
import {
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";

import { useWorkbenchStore } from "../../app/store";
import type { ProjectChatSearchResponse } from "./chatSearchDomain";
import { projectChatSearchDesktop } from "./chatSearchDesktop";
import { ChatSearchControl } from "./ChatSearchControl";
import { preloadSearchablePicker } from "../../ui/searchablePickerLoad";
import {
  clearChatSearchFolderRequest,
  requestChatSearchInFolder,
} from "./chatSearchScope";

// The picker list loads when the window first idles.
beforeAll(() => preloadSearchablePicker());

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

  it("debounces live typing, keeps the query editable, and coalesces pending searches", async () => {
    vi.useFakeTimers();
    try {
      const pending = deferred<ProjectChatSearchResponse>();
      const search = vi
        .spyOn(projectChatSearchDesktop, "search")
        .mockReturnValueOnce(pending.promise)
        .mockImplementation(async (request) => ({
          ...response,
          query: request.query,
        }));
      render(<ChatSearchControl />);
      fireEvent.click(screen.getByRole("button", { name: "Search chats" }));
      const query = screen.getByRole("combobox", { name: "Search chats" });
      expect(query).toHaveFocus();
      fireEvent.change(query, { target: { value: "ne" } });
      await act(() => vi.advanceTimersByTimeAsync(100));
      fireEvent.change(query, { target: { value: "needle" } });
      await act(() => vi.advanceTimersByTimeAsync(199));
      expect(search).not.toHaveBeenCalled();
      await act(() => vi.advanceTimersByTimeAsync(1));
      expect(search).toHaveBeenCalledTimes(1);
      expect(query).toBeEnabled();
      fireEvent.change(query, { target: { value: "latest" } });
      await act(() => vi.advanceTimersByTimeAsync(500));
      expect(search).toHaveBeenCalledTimes(1);
      await act(async () => pending.resolve(response));
      expect(
        screen.queryByRole("listbox", { name: "Chat search results" }),
      ).toBeNull();
      await act(() => vi.advanceTimersByTimeAsync(200));
      expect(search).toHaveBeenCalledTimes(2);
      expect(search.mock.calls[1][0].query).toBe("latest");
      const results = screen.getAllByRole("option");
      const listbox = screen.getByRole("listbox", {
        name: "Chat search results",
      });
      expect(query).toHaveAttribute("aria-expanded", "true");
      expect(query).toHaveAttribute("aria-controls", listbox.id);
      expect(query).not.toHaveAttribute("aria-activedescendant");
      fireEvent.keyDown(query, { key: "ArrowDown" });
      expect(query).toHaveAttribute("aria-activedescendant", results[0].id);
      expect(results[0]).toHaveAttribute("aria-selected", "true");
      fireEvent.keyDown(query, { key: "ArrowDown" });
      expect(query).toHaveAttribute("aria-activedescendant", results[1].id);
      expect(results[0]).toHaveAttribute("aria-selected", "false");
      fireEvent.keyDown(query, { key: "ArrowUp" });
      fireEvent.keyDown(query, { key: "ArrowUp" });
      expect(query).toHaveAttribute(
        "aria-activedescendant",
        results.at(-1)!.id,
      );
      expect(fireEvent.keyDown(query, { key: "Home" })).toBe(true);
      expect(query).toHaveFocus();
      expect(results.every((result) => !result.hasAttribute("tabindex"))).toBe(
        true,
      );
      fireEvent.change(query, { target: { value: "  " } });
      await act(() => vi.advanceTimersByTimeAsync(500));
      expect(search).toHaveBeenCalledTimes(3);
      expect(search.mock.calls[2][0].query).toBe("");
    } finally {
      vi.useRealTimers();
    }
  });

  it("browses hidden attached and archived chats without guessing query text", async () => {
    const search = vi
      .spyOn(projectChatSearchDesktop, "search")
      .mockImplementation(async (request) => ({
        ...response,
        query: request.query,
        results: [
          {
            chatId: "chat-old",
            title: "Hidden archived chat",
            attached: false,
            matchedTitle: true,
            snippet: null,
          },
        ],
      }));
    const restoreChat = vi.fn(async () => true);
    useWorkbenchStore.setState({ restoreChat });
    render(<ChatSearchControl />);
    act(() => requestChatSearchInFolder("project-a"));
    expect(screen.getByRole("combobox", { name: "Search chats" })).toHaveValue(
      "",
    );
    fireEvent.click(
      await screen.findByRole("option", { name: /^Hidden archived chat/ }),
    );
    expect(search).toHaveBeenCalledWith({
      projectId: "project-a",
      projectGeneration: 3,
      expectedTransitionEpoch: 2,
      query: "",
    });
    expect(restoreChat).toHaveBeenCalledWith("chat-old", "project-a");
    expect(screen.queryByRole("dialog", { name: "Search chats" })).toBeNull();
  });

  it("restarts a mounted palette with query focus and invalidates its pending search", async () => {
    const pending = deferred<typeof response>();
    const search = vi
      .spyOn(projectChatSearchDesktop, "search")
      .mockReturnValue(pending.promise);
    render(<ChatSearchControl />);
    fireEvent.click(screen.getByRole("button", { name: "Search chats" }));
    const query = screen.getByRole("combobox", { name: "Search chats" });
    fireEvent.change(query, { target: { value: "needle" } });
    fireEvent.submit(query.closest("form")!);
    screen.getByRole("button", { name: "Close" }).focus();
    act(() => requestChatSearchInFolder("project-a"));
    expect(query).toHaveFocus();
    expect(query).toHaveValue("");
    await act(async () => pending.resolve(response));
    expect(
      screen.queryByRole("listbox", { name: "Chat search results" }),
    ).toBeNull();
    expect(search).toHaveBeenCalledTimes(1);
  });

  it("supports immediate Enter, edits criteria, and navigates attached and archived results", async () => {
    const search = vi
      .spyOn(projectChatSearchDesktop, "search")
      .mockResolvedValue(response);
    const activateChat = vi.fn().mockResolvedValue(undefined);
    const restoreChat = vi.fn().mockResolvedValue(true);
    useWorkbenchStore.setState({ activateChat, restoreChat });
    render(<ChatSearchControl />);

    const trigger = screen.getByRole("button", { name: "Search chats" });
    fireEvent.click(trigger);
    const dialog = screen.getByRole("dialog", { name: "Search chats" });
    const input = screen.getByRole("combobox", { name: "Search chats" });
    expect(input).toHaveFocus();
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
    fireEvent.click(screen.getByRole("option", { name: "B" }));
    expect(activateChat).toHaveBeenCalledWith("chat-b");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    fireEvent.click(trigger);
    const reopenedInput = screen.getByRole("combobox", {
      name: "Search chats",
    });
    fireEvent.change(reopenedInput, { target: { value: "needle" } });
    fireEvent.submit(reopenedInput.closest("form")!);
    await screen.findByRole("listbox", { name: "Chat search results" });
    fireEvent.click(screen.getByRole("option", { name: "Old — Archived" }));
    expect(restoreChat).toHaveBeenCalledWith("chat-old", "project-a");
    expect(dialog).not.toBeInTheDocument();
  });

  it("opens the active result with Enter and searches with Enter while none is active", async () => {
    const search = vi
      .spyOn(projectChatSearchDesktop, "search")
      .mockResolvedValue(response);
    const restoreChat = vi.fn().mockResolvedValue(true);
    useWorkbenchStore.setState({ restoreChat });
    render(<ChatSearchControl />);
    fireEvent.click(screen.getByRole("button", { name: "Search chats" }));
    const input = screen.getByRole("combobox", { name: "Search chats" });
    fireEvent.change(input, { target: { value: "needle" } });
    fireEvent.submit(input.closest("form")!);
    await screen.findByRole("listbox", { name: "Chat search results" });
    expect(search).toHaveBeenCalledTimes(1);

    expect(fireEvent.keyDown(input, { key: "Enter" })).toBe(true);
    expect(fireEvent.mouseDown(screen.getByRole("option", { name: "B" }))).toBe(
      false,
    );
    fireEvent.keyDown(input, { key: "ArrowDown", isComposing: true });
    expect(input).not.toHaveAttribute("aria-activedescendant");

    fireEvent.keyDown(input, { key: "ArrowUp" });
    expect(input).toHaveAttribute(
      "aria-activedescendant",
      screen.getByRole("option", { name: "Old — Archived" }).id,
    );
    expect(fireEvent.keyDown(input, { key: "ArrowDown", shiftKey: true })).toBe(
      true,
    );
    expect(fireEvent.keyDown(input, { key: "ArrowDown", altKey: true })).toBe(
      true,
    );
    fireEvent.change(input, { target: { value: "needles" } });
    fireEvent.submit(input.closest("form")!);
    await screen.findByRole("listbox", { name: "Chat search results" });
    expect(search).toHaveBeenCalledTimes(2);
    expect(input).not.toHaveAttribute("aria-activedescendant");
    expect(
      screen
        .getAllByRole("option")
        .every((option) => option.getAttribute("aria-selected") === "false"),
    ).toBe(true);

    fireEvent.keyDown(input, { key: "ArrowUp" });
    expect(fireEvent.keyDown(input, { key: "Enter" })).toBe(false);
    expect(restoreChat).toHaveBeenCalledWith("chat-old", "project-a");
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
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
    const firstInput = await screen.findByRole("combobox", {
      name: "Search chats",
    });
    fireEvent.change(firstInput, { target: { value: "needle" } });
    fireEvent.submit(firstInput.closest("form")!);
    await screen.findByRole("listbox", { name: "Chat search results" });
    fireEvent.click(screen.getByRole("option", { name: "B" }));
    await waitFor(() => expect(activateChat).toHaveBeenCalledWith("chat-b"));
    expect(priorReturn).not.toHaveBeenCalled();
    expect(activated).toHaveFocus();

    const nextReturn = vi.fn();
    act(() => requestChatSearchInFolder("project-a", nextReturn));
    await screen.findByRole("dialog", { name: "Search chats" });
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
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
    expect(screen.queryByRole("dialog", { name: "Search chats" })).toBeNull();

    act(() => useWorkbenchStore.setState({ projectTransitioning: false }));
    const trigger = screen.getByRole("button", { name: "Search chats" });
    fireEvent.click(trigger);
    fireEvent.click(screen.getByRole("button", { name: "Close" }));

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
    const input = screen.getByRole("combobox", { name: "Search chats" });
    fireEvent.change(input, { target: { value: "Research" } });
    fireEvent.submit(input.closest("form")!);
    expect(
      await screen.findByRole("option", { name: /^Research/ }),
    ).toBeInTheDocument();
    expect(search).toHaveBeenCalledWith({
      projectId: "cross",
      projectGeneration: 3,
      expectedTransitionEpoch: 2,
      query: "Research",
    });
    fireEvent.click(
      screen.getByRole("option", { name: /^Research — .*Archived$/ }),
    );
    expect(restoreChat).toHaveBeenCalledWith("chat-old", "cross");
    expect(switchProject).not.toHaveBeenCalled();
    expect(useWorkbenchStore.getState().project?.id).toBe("project-a");
  });

  it("opens an inactive project's hidden attached result through authoritative project activation", async () => {
    const openRailChat = vi.fn(async () => undefined);
    const restoreChat = vi.fn(async () => true);
    useWorkbenchStore.setState({
      openRailChat,
      restoreChat,
      projects: [
        ...useWorkbenchStore.getState().projects,
        {
          id: "cross",
          name: "Cross",
          rootCount: 1,
          lastOpenedAt: 1,
          railChats: Array.from({ length: 8 }, (_, position) => ({
            id: `visible-${position}`,
            title: `Visible ${position}`,
            pinned: false,
            position,
          })),
          railChatsTruncated: true,
        },
      ],
    });
    vi.spyOn(projectChatSearchDesktop, "search").mockImplementation(
      async (request) => ({
        ...response,
        projectId: request.projectId,
        query: request.query,
        results: [
          {
            chatId: "hidden-ninth",
            title: "Hidden ninth chat",
            attached: true,
            matchedTitle: true,
            snippet: null,
          },
        ],
      }),
    );
    render(<ChatSearchControl />);
    act(() => requestChatSearchInFolder("cross"));
    fireEvent.click(
      await screen.findByRole("option", { name: /^Hidden ninth chat/ }),
    );
    expect(openRailChat).toHaveBeenCalledWith("cross", "hidden-ninth");
    expect(restoreChat).not.toHaveBeenCalled();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.queryByRole("dialog", { name: "Search chats" })).toBeNull();
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
    const input = screen.getByRole("combobox", { name: "Search chats" });
    fireEvent.change(input, { target: { value: "Research" } });
    fireEvent.submit(input.closest("form")!);
    expect(
      await screen.findByRole("option", { name: /^Research/ }),
    ).toHaveTextContent("Cross");
    expect(search.mock.calls.map((call) => call[0]?.projectId)).toEqual([
      "project-a",
      "cross",
    ]);
    fireEvent.click(
      screen.getByRole("option", { name: /^Research — .*Archived$/ }),
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
    const input = screen.getByRole("combobox", { name: "Search chats" });
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
    const input = screen.getByRole("combobox", { name: "Search chats" });
    fireEvent.change(input, { target: { value: "needle" } });
    fireEvent.submit(input.closest("form")!);
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
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
    const input = screen.getByRole("combobox", { name: "Search chats" });
    fireEvent.change(input, { target: { value: "needle" } });
    fireEvent.submit(input.closest("form")!);
    expect(await screen.findByText(/No chats matched/)).toBeVisible();
    expect(screen.getByRole("status")).toHaveTextContent("0 chats");

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
    fireEvent.click(screen.getByRole("option", { name: "Old — Archived" }));
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
    const dialog = screen.getByRole("dialog", { name: "Search chats" });
    const input = screen.getByRole("combobox", { name: "Search chats" });
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
    const dialog = screen.getByRole("dialog", { name: "Search chats" });
    const select = screen.getByRole("button", { name: "Search in" });
    const close = screen.getByRole("button", { name: "Close" });

    fireEvent.click(select);
    const pickerInput = screen.getByRole("combobox", { name: "Search in" });
    expect(dialog.contains(pickerInput)).toBe(true);
    await waitFor(() => expect(pickerInput).toHaveFocus());
    expect(screen.getAllByRole("option")).toHaveLength(65);
    const forwardTab = createEvent.keyDown(pickerInput, { key: "Tab" });
    fireEvent(pickerInput, forwardTab);
    expect(forwardTab.defaultPrevented).toBe(true);
    expect(screen.queryByRole("combobox", { name: "Search in" })).toBeNull();
    expect(close).toHaveFocus();
    fireEvent.click(select);
    const reverseInput = screen.getByRole("combobox", { name: "Search in" });
    await waitFor(() => expect(reverseInput).toHaveFocus());
    const reverseTab = createEvent.keyDown(reverseInput, {
      key: "Tab",
      shiftKey: true,
    });
    fireEvent(reverseInput, reverseTab);
    expect(reverseTab.defaultPrevented).toBe(false);
    expect(screen.queryByRole("combobox", { name: "Search in" })).toBeNull();
    fireEvent.click(select);
    const escapeInput = screen.getByRole("combobox", { name: "Search in" });
    await waitFor(() => expect(escapeInput).toHaveFocus());
    fireEvent.keyDown(escapeInput, { key: "Escape" });
    expect(screen.queryByRole("combobox", { name: "Search in" })).toBeNull();
    expect(dialog).toBeVisible();
    expect(select).toHaveFocus();

    select.focus();
    fireEvent.keyDown(dialog, { key: "Tab" });
    expect(close).toHaveFocus();

    close.focus();
    fireEvent.keyDown(dialog, { key: "Tab", shiftKey: true });
    expect(select).toHaveFocus();
  });

  it("names rows by Chat title and labels only the archived ones", async () => {
    vi.spyOn(projectChatSearchDesktop, "search").mockResolvedValue(response);
    render(<ChatSearchControl />);
    fireEvent.click(screen.getByRole("button", { name: "Search chats" }));
    const input = screen.getByRole("combobox", { name: "Search chats" });
    fireEvent.change(input, { target: { value: "needle" } });
    fireEvent.submit(input.closest("form")!);
    const [attached, archived] = within(
      await screen.findByRole("listbox", { name: "Chat search results" }),
    ).getAllByRole("option");
    expect(attached).toHaveTextContent(/^Battached needle context$/);
    expect(archived).toHaveTextContent(/^OldArchived$/);
    expect(screen.getByRole("status")).toHaveTextContent("2 chats");
  });
});
