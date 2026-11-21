import {
  act,
  cleanup,
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
import { App } from "../../app/App";
import { closeProjectCreation } from "../../app/projectCreationState";
import { settleLazyModules } from "../../test/settleLazyModules";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { resetBrowserDesktopForTests } from "../../services/browserDesktop";
import type { ProjectTransitionResult } from "../../domain/workbench";
import type { ProjectChatSearchResponse } from "./chatSearchDomain";
import { projectChatSearchDesktop } from "./chatSearchDesktop";
import { ChatSearchControl } from "./ChatSearchControl";
import { preloadSearchablePicker } from "../../ui/searchablePickerLoad";
import {
  clearChatSearchFolderRequest,
  requestChatSearchInFolder,
} from "./chatSearchScope";

beforeAll(() => preloadSearchablePicker());

async function chatResults() {
  const group = await screen.findByRole("group", { name: "Chats" });
  await waitFor(() =>
    expect(within(group).getAllByRole("option").length).toBeGreaterThan(0),
  );
  return group;
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
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
    closeProjectCreation();
    vi.stubGlobal("matchMedia", () => ({
      matches: false,
      addEventListener() {},
      removeEventListener() {},
    }));
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    );
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

  afterEach(() => {
    clearChatSearchFolderRequest();
    vi.unstubAllGlobals();
  });

  it("keeps project creation and its folders through a real transition and native failure for retry", async () => {
    resetBrowserDesktopForTests();
    await useWorkbenchStore.getState().initialize();
    const project = getWorkbenchPorts().project;
    const pending = deferred<ProjectTransitionResult>();
    vi.spyOn(project, "createProject")
      .mockImplementation(async (request, _projectId, transitionEpoch) => ({
        committed: false,
        transitionEpoch,
        snapshot: null,
        recoveryError: `${request.name}: ${request.sourceTokens.join(",")} retry failed`,
      }))
      .mockReturnValueOnce(pending.promise);
    vi.spyOn(project, "pickProjectSourceFolder").mockResolvedValue({
      token: "source-token",
      name: "Source",
      path: "/selected/source",
    });
    render(<App />);
    await settleLazyModules();
    fireEvent.click(
      (await screen.findAllByRole("button", { name: "Search chats" }))[0],
    );
    fireEvent.click(await screen.findByRole("option", { name: "New project" }));
    const creation = await screen.findByRole("dialog", {
      name: "Create project",
    });
    fireEvent.change(
      within(creation).getByRole("textbox", { name: "Project name" }),
      { target: { value: "Retry project" } },
    );
    fireEvent.click(within(creation).getByRole("button", { name: "Add" }));
    await within(creation).findByText("Source");
    fireEvent.click(
      within(creation).getByRole("button", {
        name: "Create project",
      }),
    );
    await waitFor(() =>
      expect(useWorkbenchStore.getState().projectTransitioning).toBe(true),
    );
    expect(screen.queryByRole("dialog", { name: "Search chats" })).toBeNull();
    expect(creation).toBeInTheDocument();
    await screen.findByText("Opening Sage...");
    expect(creation).toBeInTheDocument();
    await act(async () => pending.reject(new Error("Native creation failed")));
    expect(await within(creation).findByRole("alert")).toHaveTextContent(
      "Native creation failed",
    );
    expect(within(creation).getByRole("textbox")).toHaveValue("Retry project");
    expect(
      within(creation).getByRole("button", { name: "Remove Source" }),
    ).toBeEnabled();
    fireEvent.click(
      within(creation).getByRole("button", {
        name: "Create project",
      }),
    );
    await waitFor(() =>
      expect(within(creation).getByRole("alert")).toHaveTextContent(
        "Retry project: source-token retry failed",
      ),
    );
    expect(within(creation).getByRole("textbox")).toHaveValue("Retry project");
    expect(useWorkbenchStore.getState().projectTransitioning).toBe(false);
    fireEvent.click(within(creation).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("reaches a ninth project from global scope and explicit project requests", async () => {
    useWorkbenchStore.setState({
      projects: Array.from({ length: 9 }, (_, index) => ({
        id: index === 0 ? "project-a" : `project-${index + 1}`,
        name: `Project ${index + 1}`,
        rootCount: 1,
        lastOpenedAt: 1,
      })),
    });
    vi.spyOn(projectChatSearchDesktop, "search").mockImplementation(
      async (request) => ({
        ...response,
        projectId: request.projectId,
        results:
          request.projectId === "project-9"
            ? [{ ...response.results[0], title: "Ninth project chat" }]
            : [],
      }),
    );
    render(<ChatSearchControl />);
    fireEvent.click(screen.getByRole("button", { name: "Search chats" }));
    await screen.findByText(/No chats matched/);
    fireEvent.change(screen.getByRole("combobox", { name: "Search in" }), {
      target: { value: "project-9" },
    });
    expect(
      await screen.findByRole("option", {
        name: "Ninth project chat — Project 9",
      }),
    ).toBeVisible();
    fireEvent.keyDown(screen.getByRole("dialog", { name: "Search chats" }), {
      key: "Escape",
    });
    act(() => requestChatSearchInFolder("project-9"));
    expect(
      await screen.findByRole("option", {
        name: "Ninth project chat — Project 9",
      }),
    ).toBeVisible();
    expect(screen.getByRole("combobox", { name: "Search in" })).toHaveValue(
      "project-9",
    );
  });

  it("shows healthy results with one muted failure row for an over-limit project", async () => {
    useWorkbenchStore.setState({
      projects: [
        { id: "large", name: "Large", rootCount: 1, lastOpenedAt: 1 },
        ...useWorkbenchStore.getState().projects,
      ],
    });
    vi.spyOn(projectChatSearchDesktop, "search").mockImplementation(
      async (request) => {
        if (request.projectId === "large") throw new Error("too large");
        return response;
      },
    );
    render(<ChatSearchControl />);
    fireEvent.click(screen.getByRole("button", { name: "Search chats" }));
    expect(await screen.findByRole("option", { name: "B — A" })).toBeVisible();
    expect(
      screen.getAllByText("Couldn't search Large: too large"),
    ).toHaveLength(1);
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("status")).toHaveTextContent("Partial results");
  });

  it("opens project creation from a quick action and restores query focus on cancel", async () => {
    resetBrowserDesktopForTests();
    render(<App />);
    await settleLazyModules();
    fireEvent.click(
      (await screen.findAllByRole("button", { name: "Search chats" }))[0],
    );
    const query = await screen.findByRole("combobox", { name: "Search chats" });
    fireEvent.click(screen.getByRole("option", { name: "New project" }));
    const creation = await screen.findByRole("dialog", {
      name: "Create project",
    });
    expect(within(creation).getByRole("textbox")).toHaveFocus();
    fireEvent.keyDown(creation, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Create project" })).toBeNull();
    expect(query).toHaveFocus();
  });

  it("starts a new chat draft from a quick action", async () => {
    render(<ChatSearchControl />);
    fireEvent.click(screen.getByRole("button", { name: "Search chats" }));
    fireEvent.click(await screen.findByRole("option", { name: "New chat" }));
    await waitFor(() =>
      expect(useWorkbenchStore.getState().agentCenter).toBe("draft"),
    );
    expect(screen.queryByRole("dialog", { name: "Search chats" })).toBeNull();
  });

  it("caps visible chats across projects at 64 and keeps their newest rows", async () => {
    useWorkbenchStore.setState({
      projects: [
        { id: "project-a", name: "A", rootCount: 1, lastOpenedAt: 1 },
        { id: "project-b", name: "B", rootCount: 1, lastOpenedAt: 1 },
      ],
    });
    vi.spyOn(projectChatSearchDesktop, "search").mockImplementation(
      async (request) => ({
        ...response,
        projectId: request.projectId,
        query: request.query,
        results: Array.from({ length: 64 }, (_, index) => ({
          chatId: `chat-${index}`,
          title: `${request.projectId} ${index}`,
          attached: true,
          matchedTitle: true,
          snippet: null,
          updatedAt: request.projectId === "project-b" ? index + 100 : index,
        })),
      }),
    );
    render(<ChatSearchControl />);
    fireEvent.click(screen.getByRole("button", { name: "Search chats" }));
    const group = await chatResults();
    const rows = within(group).getAllByRole("option");
    expect(rows).toHaveLength(64);
    expect(rows[0]).toHaveAccessibleName("project-b 63 — B");
    expect(rows[63]).toHaveAccessibleName("project-b 0 — B");
  });

  it("focuses a palette with quick actions and switches between actions and chats", async () => {
    vi.spyOn(projectChatSearchDesktop, "search").mockResolvedValue(response);
    render(<ChatSearchControl />);
    fireEvent.click(screen.getByRole("button", { name: "Search chats" }));
    const input = await screen.findByRole("combobox", { name: "Search chats" });
    expect(input).toHaveFocus();
    expect(input).toHaveAttribute("placeholder", "Search chats");
    expect(
      screen.getByRole("group", { name: "Quick actions" }),
    ).toHaveTextContent("New chat");
    expect(screen.getByRole("option", { name: /New project/ })).toBeVisible();
    await screen.findByRole("option", { name: /^B/ });
    fireEvent.click(screen.getByRole("tab", { name: "Actions" }));
    expect(screen.queryByRole("option", { name: /^B/ })).toBeNull();
    expect(screen.getByRole("option", { name: /New chat/ })).toBeVisible();
    fireEvent.click(screen.getByRole("tab", { name: "Chats" }));
    expect(screen.queryByRole("option", { name: /New chat/ })).toBeNull();
    expect(await screen.findByRole("option", { name: /^B/ })).toBeVisible();
    fireEvent.click(screen.getByRole("tab", { name: /^All$/ }));
    expect(screen.getByRole("option", { name: /New chat/ })).toBeVisible();
    fireEvent.keyDown(input, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it.each([false, true])(
    "includes native-owned private chats in All folders without listing internal projects (private active: %s)",
    async (privateActive) => {
      useWorkbenchStore.setState({
        ...(privateActive
          ? {
              project: {
                id: "private-owner",
                name: "Choose project",
                isInternal: true,
                roots: [],
              },
            }
          : {}),
        projects: Array.from({ length: 9 }, (_, i) => ({
          id: `public-${i}`,
          name: `Public ${i}`,
          rootCount: 1,
          lastOpenedAt: 1,
        })),
        unscopedChats: [
          {
            id: "private-owner",
            name: "",
            rootCount: 1,
            lastOpenedAt: 1,
            railArchived: [
              { id: "private-chat", title: "Chat: Private needle" },
            ],
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
              chatId:
                request.projectId === "private-owner"
                  ? "private-chat"
                  : `chat-${request.projectId}`,
              title:
                request.projectId === "private-owner"
                  ? "Chat: Private needle"
                  : "Chat: Public needle",
              attached: request.projectId !== "private-owner",
              matchedTitle: true,
              snippet: null,
            },
          ],
        }),
      );
      render(<ChatSearchControl />);
      fireEvent.click(screen.getByRole("button", { name: "Search chats" }));
      expect(screen.queryByRole("button", { name: "Search in" })).toBeNull();
      const input = await screen.findByRole("combobox", {
        name: "Search chats",
      });
      fireEvent.change(input, { target: { value: "needle" } });
      fireEvent.submit(input.closest("form")!);
      expect(
        await screen.findByRole("option", { name: /Private needle/ }),
      ).toBeInTheDocument();
      expect(
        screen.getAllByRole("option", { name: /Public needle/ }),
      ).toHaveLength(8);
    },
  );

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
      expect(screen.queryByRole("option", { name: /^B \u2014/ })).toBeNull();
      await act(() => vi.advanceTimersByTimeAsync(200));
      expect(search).toHaveBeenCalledTimes(2);
      expect(search.mock.calls[1][0].query).toBe("latest");
      const results = within(
        screen.getByRole("group", { name: "Chats" }),
      ).getAllByRole("option");
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
    const query = await screen.findByRole("combobox", { name: "Search chats" });
    fireEvent.change(query, { target: { value: "needle" } });
    fireEvent.submit(query.closest("form")!);
    await waitFor(() => expect(search).toHaveBeenCalledTimes(1));
    screen.getByRole("tab", { name: "Chats" }).focus();
    act(() => requestChatSearchInFolder("project-a"));
    expect(query).toHaveFocus();
    expect(query).toHaveValue("");
    await act(async () => pending.resolve(response));
    expect(screen.queryByRole("option", { name: /^B \u2014/ })).toBeNull();
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
    const input = await screen.findByRole("combobox", { name: "Search chats" });
    expect(input).toHaveFocus();
    fireEvent.change(input, { target: { value: "needle" } });
    expect(search).not.toHaveBeenCalled();
    fireEvent.submit(input.closest("form")!);
    expect(within(await chatResults()).getAllByRole("option")).toHaveLength(2);
    expect(search).toHaveBeenCalledWith({
      projectId: "project-a",
      projectGeneration: 3,
      expectedTransitionEpoch: 2,
      query: "needle",
    });
    fireEvent.change(input, { target: { value: "changed" } });
    expect(screen.queryByRole("option", { name: /^B \u2014/ })).toBeNull();
    fireEvent.change(input, { target: { value: "needle" } });
    fireEvent.submit(input.closest("form")!);
    await chatResults();
    fireEvent.click(screen.getByRole("option", { name: "B \u2014 A" }));
    expect(activateChat).toHaveBeenCalledWith("chat-b");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    fireEvent.click(trigger);
    const reopenedInput = screen.getByRole("combobox", {
      name: "Search chats",
    });
    fireEvent.change(reopenedInput, { target: { value: "needle" } });
    fireEvent.submit(reopenedInput.closest("form")!);
    await chatResults();
    fireEvent.click(
      screen.getByRole("option", { name: "Old \u2014 A \u2014 Archived" }),
    );
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
    const input = await screen.findByRole("combobox", { name: "Search chats" });
    fireEvent.change(input, { target: { value: "needle" } });
    fireEvent.submit(input.closest("form")!);
    await chatResults();
    expect(search).toHaveBeenCalledTimes(1);

    expect(fireEvent.keyDown(input, { key: "Enter" })).toBe(true);
    expect(
      fireEvent.mouseDown(screen.getByRole("option", { name: "B \u2014 A" })),
    ).toBe(false);
    fireEvent.keyDown(input, { key: "ArrowDown", isComposing: true });
    expect(input).not.toHaveAttribute("aria-activedescendant");

    fireEvent.keyDown(input, { key: "ArrowUp" });
    expect(input).toHaveAttribute(
      "aria-activedescendant",
      screen.getByRole("option", { name: "Old \u2014 A \u2014 Archived" }).id,
    );
    expect(fireEvent.keyDown(input, { key: "ArrowDown", shiftKey: true })).toBe(
      true,
    );
    expect(fireEvent.keyDown(input, { key: "ArrowDown", altKey: true })).toBe(
      true,
    );
    fireEvent.change(input, { target: { value: "needles" } });
    fireEvent.submit(input.closest("form")!);
    await chatResults();
    expect(search).toHaveBeenCalledTimes(2);
    expect(input).not.toHaveAttribute("aria-activedescendant");
    expect(
      within(screen.getByRole("group", { name: "Chats" }))
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
    await chatResults();
    fireEvent.click(screen.getByRole("option", { name: "B \u2014 A" }));
    await waitFor(() => expect(activateChat).toHaveBeenCalledWith("chat-b"));
    expect(priorReturn).not.toHaveBeenCalled();
    expect(activated).toHaveFocus();

    const nextReturn = vi.fn();
    act(() => requestChatSearchInFolder("project-a", nextReturn));
    await screen.findByRole("dialog", { name: "Search chats" });
    fireEvent.keyDown(screen.getByRole("dialog", { name: "Search chats" }), {
      key: "Escape",
    });
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
    fireEvent.keyDown(screen.getByRole("dialog", { name: "Search chats" }), {
      key: "Escape",
    });

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
      .mockImplementation(async (request) =>
        request.projectId === "cross"
          ? foreignResponse
          : { ...foreignResponse, projectId: request.projectId, results: [] },
      );
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
    const input = await screen.findByRole("combobox", { name: "Search chats" });
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
        results:
          request.projectId === "cross"
            ? [
                {
                  chatId: "hidden-ninth",
                  title: "Hidden ninth chat",
                  attached: true,
                  matchedTitle: true,
                  snippet: null,
                },
              ]
            : [],
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
    const input = await screen.findByRole("combobox", { name: "Search chats" });
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

  it("closes the palette when its owner becomes stale", async () => {
    render(<ChatSearchControl />);
    const trigger = screen.getByRole("button", { name: "Search chats" });
    fireEvent.click(trigger);
    fireEvent.click(screen.getByRole("tab", { name: "Chats" }));
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
    fireEvent.click(screen.getByRole("tab", { name: "Chats" }));
    const input = await screen.findByRole("combobox", { name: "Search chats" });
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
    const input = await screen.findByRole("combobox", { name: "Search chats" });
    fireEvent.change(input, { target: { value: "needle" } });
    fireEvent.submit(input.closest("form")!);
    fireEvent.keyDown(screen.getByRole("dialog", { name: "Search chats" }), {
      key: "Escape",
    });
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
    const input = await screen.findByRole("combobox", { name: "Search chats" });
    fireEvent.change(input, { target: { value: "needle" } });
    fireEvent.submit(input.closest("form")!);
    expect(await screen.findByText(/No chats matched/)).toBeVisible();
    expect(screen.getByRole("status")).toHaveTextContent("0 chats");

    fireEvent.submit(input.closest("form")!);
    await chatResults();
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
      screen.getByRole("option", { name: "Old \u2014 A \u2014 Archived" }),
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
    const dialog = screen.getByRole("dialog", { name: "Search chats" });
    const input = await screen.findByRole("combobox", { name: "Search chats" });
    fireEvent.change(input, { target: { value: "needle" } });
    fireEvent.submit(input.closest("form")!);
    expect(
      await screen.findByText("Couldn't search A: browser unavailable"),
    ).toBeVisible();
    expect(screen.getByRole("status")).toHaveTextContent(
      "Partial results: 0 chats",
    );
    expect(screen.queryByText(/No chats matched/)).toBeNull();
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
    const input = await screen.findByRole("combobox", { name: "Search chats" });
    const all = screen.getByRole("tab", { name: /^All$/ });
    const scope = screen.getByRole("combobox", { name: "Search in" });
    scope.focus();
    fireEvent.keyDown(dialog, { key: "Tab" });
    expect(input).toHaveFocus();
    fireEvent.keyDown(input, { key: "Tab", shiftKey: true });
    expect(scope).toHaveFocus();
    all.focus();
    fireEvent.keyDown(all, { key: "ArrowRight" });
    expect(screen.getByRole("tab", { name: "Actions" })).toHaveFocus();
    expect(screen.getByRole("tab", { name: "Actions" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
  });

  it("names rows by Chat title and labels only the archived ones", async () => {
    vi.spyOn(projectChatSearchDesktop, "search").mockResolvedValue(response);
    render(<ChatSearchControl />);
    fireEvent.click(screen.getByRole("button", { name: "Search chats" }));
    const input = await screen.findByRole("combobox", { name: "Search chats" });
    fireEvent.change(input, { target: { value: "needle" } });
    fireEvent.submit(input.closest("form")!);
    const [attached, archived] = within(await chatResults()).getAllByRole(
      "option",
    );
    expect(attached).toHaveTextContent(/^BA$/);
    expect(archived).toHaveTextContent(/^OldA \u00b7 Archived$/);
    expect(screen.getByRole("status")).toHaveTextContent("2 chats");
  });
});
