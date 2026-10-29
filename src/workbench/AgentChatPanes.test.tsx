import { Profiler } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { clearAcpProjectionRegistry } from "../app/acpProjectionRegistry";
import { acpProjectionStoreFor } from "../app/acpProjectionRegistry";
import { pinnedAgentChatOwner } from "../app/agentChatOwner";
import { settleLazyModules } from "../test/settleLazyModules";
import { useWorkbenchStore } from "../app/store";
import { openAgentArtifact } from "../app/openAgentArtifact";
import type {
  AcpChatPage,
  AcpItem,
  AcpItemIdentity,
  ProjectionRevision,
  TurnProjection,
} from "../domain/acp";
import type { WorkbenchTab } from "../domain/workbench";
import { AgentChatPanes } from "./AgentChatPanes";
import { launchSideChat } from "./AgentToolPanel";
import { AgentTaskHeader } from "./AgentTaskHeader";
import { SIDE_CHAT_SPLIT, useSideChatState } from "./sideChatState";
import { WORKBENCH_LAYOUT_STORAGE_KEY } from "./workbenchLayoutStorage";
import type { AgentSessionChrome } from "../app/agentChatOwner";
import {
  applyBrowseIntent,
  browseSessionForOwner,
  clearBrowseSessions,
  harnessBrowseOwner,
  setHarnessBrowseOwner,
  type HarnessBrowseOwner,
} from "../features/preview/harnessBrowse";

let resizeCallback: ResizeObserverCallback | null = null;
const disconnect = vi.fn();

vi.mock("./AgentChatPane", () => ({
  AgentChatPane: ({
    tab,
    onFocus,
  }: {
    tab: WorkbenchTab;
    onFocus: () => void;
  }) => (
    <button data-testid={`chat-pane-${tab.id}`} onClick={onFocus}>
      {tab.title}
    </button>
  ),
}));

vi.mock("../features/chat/transcriptLazy", () => ({
  TranscriptPart: () => null,
}));

vi.mock("../app/openAgentArtifact", () => ({
  openAgentArtifact: vi.fn(async () => true),
}));

const initialState = useWorkbenchStore.getState();
const tabs: WorkbenchTab[] = [
  {
    id: "chat:a",
    kind: "chat",
    title: "Chat A",
    position: 0,
    pinned: false,
    dirty: false,
  },
  {
    id: "chat:b",
    kind: "chat",
    title: "Chat B",
    position: 1,
    pinned: false,
    dirty: false,
  },
];

function reportCenterWidth(width: number) {
  act(() => {
    resizeCallback?.(
      [
        {
          contentRect: { width },
        } as ResizeObserverEntry,
      ],
      {} as ResizeObserver,
    );
  });
}

function seedPreviewOwner(chatId: string, sessionId: string): void {
  const store = acpProjectionStoreFor(chatId);
  const scope = {
    projectId: "project-a",
    projectGeneration: "7",
    chatId,
  } as never;
  store.getState().activateScope(scope);
  store.getState().applyAuthoritativePage(scope, {
    projectId: "project-a",
    chatId,
    title: chatId,
    projectionRevision: "1",
    currentSessionId: sessionId,
    sessions: [{ id: sessionId, runtimeEpoch: "3" }],
    turns: [],
    nextCursor: null,
  } as never);
}

beforeEach(() => {
  useSideChatState.setState({ sides: [], pairedId: null });
  vi.mocked(openAgentArtifact).mockClear();
  localStorage.clear();
  clearAcpProjectionRegistry();
  clearBrowseSessions();
  disconnect.mockClear();
  resizeCallback = null;
  vi.stubGlobal(
    "ResizeObserver",
    class ResizeObserverMock {
      constructor(callback: ResizeObserverCallback) {
        resizeCallback = callback;
      }

      observe() {}
      unobserve() {}
      disconnect() {
        disconnect();
      }
    },
  );
  useWorkbenchStore.setState({
    ...initialState,
    tabs,
    activeTabId: "chat:b",
    activateTab: vi.fn(async (tabId: string) => {
      useWorkbenchStore.setState({ activeTabId: tabId });
    }),
  });
});

afterEach(() => {
  cleanup();
  localStorage.clear();
  clearBrowseSessions();
  clearAcpProjectionRegistry();
  vi.unstubAllGlobals();
  useWorkbenchStore.setState(initialState, true);
});

describe("AgentChatPanes measured focus-only fallback", () => {
  it("keeps the project home free of header chrome when no Chat is open", () => {
    useWorkbenchStore.setState({ tabs: [], activeTabId: null });
    const onArtifactHeaderMount = vi.fn();
    const { container } = render(
      <AgentChatPanes onArtifactHeaderMount={onArtifactHeaderMount} />,
    );
    expect(screen.getByRole("tabpanel")).toHaveClass("active-canvas");
    expect(container.querySelector(".agent-chat-center__header")).toBeNull();
    expect(onArtifactHeaderMount).not.toHaveBeenCalled();
  });

  it("uses the sidebar for chat switching without a duplicate chat tab row", () => {
    render(<AgentChatPanes />);
    expect(screen.queryByRole("tablist", { name: "Chat tabs" })).toBeNull();
    expect(screen.queryByRole("button", { name: "New chat" })).toBeNull();
    expect(screen.getByTestId("chat-pane-chat:b")).toBeInTheDocument();
  });

  it("does not rerender for another project's side-chat status", () => {
    const onRender = vi.fn();
    render(
      <Profiler id="panes" onRender={onRender}>
        <AgentChatPanes />
      </Profiler>,
    );
    onRender.mockClear();
    act(() =>
      useSideChatState.setState({
        sides: [
          {
            id: "side-other",
            projectId: "other",
            parentChatId: "other-chat",
            running: true,
          },
        ],
      }),
    );
    expect(onRender).not.toHaveBeenCalled();
  });

  it("never grows the visible chat set across 860px or after the right pane collapses", async () => {
    render(<AgentChatPanes />);
    for (const width of [859, 860, 1000, 600, 1200]) {
      reportCenterWidth(width);
      expect(
        screen.getAllByTestId(/chat-pane-/).map((pane) => pane.dataset.testid),
      ).toEqual(["chat-pane-chat:b"]);
      expect(screen.queryByRole("separator")).toBeNull();
    }

    await act(async () => {
      await useWorkbenchStore.getState().activateTab("chat:a");
    });
    expect(screen.getByTestId("chat-pane-chat:a")).toBeInTheDocument();
    expect(screen.queryByTestId("chat-pane-chat:b")).toBeNull();

    reportCenterWidth(1_000);
    expect(
      screen.getAllByTestId(/chat-pane-/).map((pane) => pane.dataset.testid),
    ).toEqual(["chat-pane-chat:a"]);
    expect(screen.queryByRole("separator")).toBeNull();
  });

  it("replaces the center when New chat adds and activates a tab", () => {
    render(<AgentChatPanes />);
    reportCenterWidth(1200);
    act(() =>
      useWorkbenchStore.setState({
        tabs: [...tabs, { ...tabs[0]!, id: "chat:new", title: "New chat" }],
        activeTabId: "chat:new",
      }),
    );
    expect(
      screen.getAllByTestId(/chat-pane-/).map((pane) => pane.dataset.testid),
    ).toEqual(["chat-pane-chat:new"]);
    reportCenterWidth(859);
    reportCenterWidth(1200);
    expect(screen.getAllByTestId(/chat-pane-/)).toHaveLength(1);
  });

  it("does not observe center width at all for a single chat", () => {
    const view = render(<AgentChatPanes />);
    view.unmount();
    expect(disconnect).not.toHaveBeenCalled();
    expect(resizeCallback).toBeNull();
  });

  it("retains both explicitly visible Chat browse sessions across a focused page update", () => {
    useWorkbenchStore.setState({
      tabs: [tabs[0]!, { ...tabs[1]!, id: "side-b" }],
      activeTabId: "side-b",
      project: { id: "project-a", name: "A", roots: [] },
      projectGeneration: 7,
      nativeTransitionEpoch: 7,
      projectTransitioning: false,
    });
    seedPreviewOwner("chat:a", "session-a");
    seedPreviewOwner("side-b", "session-b");
    useSideChatState.setState({
      sides: [{ id: "side-b", parentChatId: "chat:a", projectId: "project-a" }],
      pairedId: "side-b",
    });
    const ownerA: HarnessBrowseOwner = {
      projectId: "project-a" as HarnessBrowseOwner["projectId"],
      projectGeneration: "7" as HarnessBrowseOwner["projectGeneration"],
      transitionEpoch: 7,
      chatId: "chat:a" as HarnessBrowseOwner["chatId"],
      sessionId: "session-a" as HarnessBrowseOwner["sessionId"],
      runtimeEpoch: "3" as HarnessBrowseOwner["runtimeEpoch"],
    };
    const ownerB: HarnessBrowseOwner = {
      ...ownerA,
      chatId: "side-b" as HarnessBrowseOwner["chatId"],
      sessionId: "session-b" as HarnessBrowseOwner["sessionId"],
    };
    render(<AgentChatPanes />);
    expect(harnessBrowseOwner()).toEqual(ownerB);
    setHarnessBrowseOwner(ownerA, ["chat:a", "side-b"]);
    applyBrowseIntent({ owner: ownerA, url: "http://127.0.0.1:4173/a" });
    setHarnessBrowseOwner(ownerB, ["chat:a", "side-b"]);
    applyBrowseIntent({ owner: ownerB, url: "http://127.0.0.1:4173/b" });

    const focused = acpProjectionStoreFor("side-b");
    const scope = focused.getState().scope;
    const page = focused.getState().page;
    if (!scope || !page) throw new Error("Focused Chat projection was absent.");
    const updatedPage: AcpChatPage = {
      ...page,
      projectionRevision: "2" as ProjectionRevision,
    };
    focused.getState().applyAuthoritativePage(scope, updatedPage);

    expect(browseSessionForOwner(ownerA)?.href).toBe("http://127.0.0.1:4173/a");
    expect(browseSessionForOwner(ownerB)?.href).toBe("http://127.0.0.1:4173/b");
  });

  it("keeps Chat B focused while its shared Editor resource becomes active", () => {
    render(<AgentChatPanes />);
    expect(pinnedAgentChatOwner()).toBe("chat:b");

    act(() => {
      useWorkbenchStore.setState({
        tabs: [
          ...tabs,
          {
            id: "file:b",
            kind: "file",
            title: "b.ts",
            position: 2,
            pinned: false,
            dirty: false,
            resource: { rootId: "root-a", relativePath: "b.ts" },
          },
        ],
        activeTabId: "file:b",
      });
    });
    expect(pinnedAgentChatOwner()).toBe("chat:b");

    act(() => {
      useWorkbenchStore.setState({ tabs: [tabs[0]!] });
    });
    expect(pinnedAgentChatOwner()).toBe("chat:a");
  });
});

describe("AgentChatPanes browse ownership across independent generation counters", () => {
  it("establishes the browse owner when projectGeneration and nativeTransitionEpoch diverge", () => {
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      projectGeneration: 7,
      nativeTransitionEpoch: 0,
      projectTransitioning: false,
    });
    seedPreviewOwner("chat:b", "session-b");

    render(<AgentChatPanes />);

    expect(harnessBrowseOwner()).toEqual({
      projectId: "project-a",
      projectGeneration: "7",
      transitionEpoch: 0,
      chatId: "chat:b",
      sessionId: "session-b",
      runtimeEpoch: "3",
    });
  });

  it("withholds the browse owner when the projection scope is a stale generation", () => {
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      projectGeneration: 8,
      nativeTransitionEpoch: 0,
      projectTransitioning: false,
    });
    seedPreviewOwner("chat:b", "session-b");

    render(<AgentChatPanes />);

    expect(harnessBrowseOwner()).toBeNull();
  });
});

describe("AgentTaskHeader working status", () => {
  it("keeps session actions and live status reachable from the header", async () => {
    const closeSession = vi.fn();
    const endSession = vi.fn();
    const chrome: AgentSessionChrome = {
      chatId: "chat:a",
      harnessStatus: "Live",
      canCloseSession: true,
      canEndSession: true,
      busy: false,
      acpPage: null,
      messages: [],
      actions: {
        current: { closeSession, endSession, focusComposer: vi.fn() },
      },
    };
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "Sage", roots: [] },
      projects: [{ id: "project-a", name: "Sage", roots: [] }],
      activeTabId: "chat:a",
    } as never);
    render(
      <AgentTaskHeader
        tab={headerTab}
        chatCount={2}
        snapshot={null}
        sessionChrome={chrome}
      />,
    );
    expect(screen.getByLabelText("Harness status: Live")).toHaveTextContent(
      "Live",
    );
    fireEvent.click(screen.getByRole("button", { name: /Chat actions for/ }));
    fireEvent.click(
      screen.getByRole("menuitem", { name: "More chat actions in Sage" }),
    );
    expect(
      await screen.findByRole("menuitem", { name: "Hand off chat" }),
    ).toBeEnabled();
    fireEvent.click(screen.getByRole("menuitem", { name: "Close session" }));
    expect(closeSession).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: /Chat actions for/ }));
    fireEvent.click(
      screen.getByRole("menuitem", { name: "More chat actions in Sage" }),
    );
    fireEvent.click(screen.getByRole("menuitem", { name: "End session" }));
    expect(endSession).toHaveBeenCalledTimes(1);
  });

  it("keeps the handoff dialog mounted after the header menu closes", async () => {
    const chrome: AgentSessionChrome = {
      chatId: "chat:a",
      harnessStatus: "Live",
      canCloseSession: false,
      canEndSession: false,
      busy: false,
      acpPage: null,
      messages: [],
      actions: {
        current: {
          closeSession: vi.fn(),
          endSession: vi.fn(),
          focusComposer: vi.fn(),
        },
      },
    };
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "Sage", roots: [] },
      projects: [{ id: "project-a", name: "Sage", roots: [] }],
      activeTabId: "chat:a",
    } as never);
    render(
      <AgentTaskHeader
        tab={headerTab}
        chatCount={2}
        snapshot={null}
        sessionChrome={chrome}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /Chat actions for/ }));
    fireEvent.click(
      screen.getByRole("menuitem", { name: "More chat actions in Sage" }),
    );
    fireEvent.click(
      await screen.findByRole("menuitem", { name: "Hand off chat" }),
    );
    expect(
      await screen.findByRole("dialog", { name: "Hand off this Chat" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /Chat actions for/ }),
    ).toHaveAttribute("aria-expanded", "false");
    fireEvent.keyDown(
      screen.getByRole("dialog", { name: "Hand off this Chat" }),
      { key: "Escape" },
    );
    await vi.waitFor(() =>
      expect(
        screen.getByRole("button", { name: /Chat actions for/ }),
      ).toHaveFocus(),
    );
  });

  it("opens and dismisses the Outputs, Subagents, and Sources panel", async () => {
    renderHeader(null);
    fireEvent.click(
      screen.getByRole("button", { name: "Show chat resources" }),
    );
    expect(
      await screen.findByRole("dialog", { name: "Chat resources" }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("dialog", { name: "Chat resources" }),
    ).toHaveFocus();
    expect(screen.getByText("Outputs")).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Outputs" })).toHaveTextContent(
      "Create a file or site",
    );
    expect(
      within(screen.getByRole("region", { name: "Outputs" })).queryByText("0"),
    ).toBeNull();
    expect(screen.queryByRole("region", { name: "Subagents" })).toBeNull();
    expect(screen.getByText("Sources")).toBeInTheDocument();
    expect(
      within(screen.getByRole("region", { name: "Sources" })).queryByText("0"),
    ).toBeNull();
    fireEvent.keyDown(screen.getByRole("dialog", { name: "Chat resources" }), {
      key: "Escape",
    });
    expect(screen.queryByRole("dialog", { name: "Chat resources" })).toBeNull();
    expect(
      screen.getByRole("button", { name: "Show chat resources" }),
    ).toHaveFocus();
  });

  function itemIdentity(id: string, ordinal: number): AcpItemIdentity {
    return {
      id: id as AcpItemIdentity["id"],
      projectId: "project-a" as AcpItemIdentity["projectId"],
      chatId: "chat:a" as AcpItemIdentity["chatId"],
      sessionId: "session-1" as AcpItemIdentity["sessionId"],
      turnId: "turn-1" as AcpItemIdentity["turnId"],
      runtimeEpoch: "3" as AcpItemIdentity["runtimeEpoch"],
      ordinal,
      origin: "primary",
      activityId: null,
      lifecycle: "completed",
      revision: "1" as AcpItemIdentity["revision"],
      createdAt: 1,
      updatedAt: 2,
      terminalAt: 2,
    };
  }

  function collaborationItem(
    id: string,
    ordinal: number,
    operation: string,
    agentKeys: readonly string[],
    agentStates: readonly (readonly [string, string])[],
    agentKey: string | null = null,
    receiverLabels: readonly string[] = [],
  ): Extract<AcpItem, { kind: "collaboration" }> {
    return {
      ...itemIdentity(id, ordinal),
      kind: "collaboration",
      toolState: null,
      payload: {
        activityId: "activity-1" as Extract<
          AcpItem,
          { kind: "collaboration" }
        >["payload"]["activityId"],
        label: operation,
        summary: "",
        receiverLabels,
        operation,
        agentKey,
        agentKeys,
        agentStates,
      },
    };
  }

  function subagentTool(
    id: string,
    ordinal: number,
    toolState: Extract<AcpItem, { kind: "toolCall" }>["toolState"],
  ): Extract<AcpItem, { kind: "toolCall" }> {
    return {
      ...itemIdentity(id, ordinal),
      lifecycle:
        toolState === "completed"
          ? "completed"
          : toolState === "failed"
            ? "failed"
            : toolState === "declined" || toolState === "cancelled"
              ? "cancelled"
              : "started",
      terminalAt:
        toolState === "pending" ||
        toolState === "inProgress" ||
        toolState === "waitingForApproval"
          ? null
          : 2,
      kind: "toolCall",
      toolState,
      payload: {
        presentation: { kind: "subagent", label: "Subagent", activityId: null },
        inputSummary: null,
      },
    };
  }

  function seedHeaderItems(items: readonly AcpItem[]) {
    seedPreviewOwner("chat:a", "session-1");
    const store = acpProjectionStoreFor("chat:a");
    const scope = store.getState().scope;
    const page = store.getState().page;
    if (!scope || !page) throw new Error("Chat projection was absent.");
    const turn: TurnProjection = {
      id: "turn-1" as TurnProjection["id"],
      sessionId: "session-1" as TurnProjection["sessionId"],
      runtimeEpoch: "3" as TurnProjection["runtimeEpoch"],
      ordinal: 0,
      state: "completed",
      revision: "1" as TurnProjection["revision"],
      createdAt: 1,
      updatedAt: 2,
      terminalAt: 2,
      items,
      activities: [],
      browserOperations: [],
      browserOperationsCollapsed: 0,
    };
    act(() => {
      store.getState().applyAuthoritativePage(scope, {
        ...page,
        projectionRevision: "2" as ProjectionRevision,
        turns: [turn],
      });
    });
  }

  it("counts separate verified spawns even when a thread ID is reused", async () => {
    seedHeaderItems([
      collaborationItem(
        "spawn-1",
        0,
        "spawnAgent",
        ["child-1"],
        [["child-1", "running"]],
      ),
      collaborationItem(
        "spawn-2",
        1,
        "spawnAgent",
        ["child-1"],
        [["child-1", "pendingInit"]],
      ),
      collaborationItem(
        "wait-1",
        2,
        "wait",
        ["child-1"],
        [["child-1", "completed"]],
      ),
    ]);
    renderHeader(null);
    fireEvent.click(
      screen.getByRole("button", { name: "Show chat resources" }),
    );
    expect(
      await screen.findByLabelText("Recent subagents working: 1"),
    ).toBeInTheDocument();
    expect(
      screen.getByLabelText("Recent subagents done: 1"),
    ).toBeInTheDocument();
  });

  it("shows named recent subagents with distinct working and done states", async () => {
    seedHeaderItems([
      collaborationItem(
        "spawn-1",
        0,
        "spawnAgent",
        ["private-thread-a", "private-thread-b"],
        [
          ["private-thread-a", "running"],
          ["private-thread-b", "running"],
        ],
        null,
        ["Researcher", "Builder"],
      ),
      collaborationItem(
        "wait-1",
        1,
        "wait",
        ["private-thread-b"],
        [["private-thread-b", "completed"]],
      ),
    ]);
    renderHeader(null);
    fireEvent.click(
      screen.getByRole("button", { name: "Show chat resources" }),
    );
    const section = await screen.findByRole("region", { name: "Subagents" });
    expect(section).toHaveTextContent("1 working");
    expect(section).toHaveTextContent("1 done");
    expect(await within(section).findByText("Researcher")).toBeInTheDocument();
    expect(await within(section).findByText("Builder")).toBeInTheDocument();
    expect(section).not.toHaveTextContent("private-thread");
  });

  it("opens a safe output file and leaves unavailable output inert", () => {
    seedHeaderItems([
      {
        ...itemIdentity("output-1", 0),
        kind: "fileChangeDiff",
        toolState: null,
        payload: {
          changes: [
            {
              resource: {
                kind: "resource",
                resource: { rootId: "root-a", relativePath: "report.md" },
              },
              originalResource: null,
              operation: "created",
              summary: "Created report",
              additions: 3,
              deletions: 0,
            },
            {
              resource: {
                kind: "unavailable",
                reason: "foreign",
                label: "private.txt",
              },
              originalResource: null,
              operation: "created",
              summary: "Unavailable",
              additions: 1,
              deletions: 0,
            },
          ],
          truncated: false,
        },
      },
    ]);
    renderHeader(null);
    fireEvent.click(
      screen.getByRole("button", { name: "Show chat resources" }),
    );
    expect(
      within(screen.getByRole("region", { name: "Outputs" })).queryByText("1"),
    ).toBeNull();
    fireEvent.click(
      screen.getByRole("button", { name: "Open output report.md" }),
    );
    expect(openAgentArtifact).toHaveBeenCalledWith({
      owner: {
        projectId: "project-a",
        chatId: "chat:a",
        sessionId: "session-1",
      },
      target: "file",
      resource: { rootId: "root-a", relativePath: "report.md" },
    });
    expect(screen.queryByRole("button", { name: /private.txt/ })).toBeNull();
  });

  it("limits resource projection to the latest 256 items across turns", () => {
    seedHeaderItems([
      {
        ...itemIdentity("old-output", 0),
        kind: "fileChangeDiff",
        toolState: null,
        payload: {
          changes: [
            {
              resource: {
                kind: "resource",
                resource: { rootId: "root-a", relativePath: "old-report.md" },
              },
              originalResource: null,
              operation: "created",
              summary: "Old report",
              additions: 1,
              deletions: 0,
            },
          ],
          truncated: false,
        },
      },
    ]);
    const store = acpProjectionStoreFor("chat:a");
    const { scope, page } = store.getState();
    if (!scope || !page) throw new Error("Chat projection was absent.");
    const newer: TurnProjection = {
      ...page.turns[0],
      id: "turn-2" as TurnProjection["id"],
      ordinal: 1,
      items: Array.from({ length: 256 }, (_, index): AcpItem => ({
        ...itemIdentity(`recent-${index}`, index),
        turnId: "turn-2" as AcpItemIdentity["turnId"],
        kind: "text",
        toolState: null,
        payload: { role: "assistant", text: "recent" },
      })),
    };
    act(() => {
      store.getState().applyAuthoritativePage(scope, {
        ...page,
        projectionRevision: "3" as ProjectionRevision,
        turns: [page.turns[0], newer],
      });
    });
    renderHeader(null);
    fireEvent.click(
      screen.getByRole("button", { name: "Show chat resources" }),
    );
    expect(
      screen.queryByRole("button", { name: "Open output old-report.md" }),
    ).toBeNull();
  });

  it("shows a recorded browser source host without rebinding its existing tab", () => {
    seedHeaderItems([]);
    const store = acpProjectionStoreFor("chat:a");
    const { scope, page } = store.getState();
    if (!scope || !page) throw new Error("Chat projection was absent.");
    act(() => {
      store.getState().applyAuthoritativePage(scope, {
        ...page,
        projectionRevision: "3" as ProjectionRevision,
        turns: [
          {
            ...page.turns[0],
            browserOperations: [
              {
                projectId: "project-a",
                chatId: "chat:a",
                sessionId: "session-1",
                turnId: "turn-1",
                tabId: "browser-1",
                operationId: "operation-1",
                operation: "openTab",
                phase: "completed",
                origin: "https://example.com/article?q=private",
                summary: "Opened page",
                createdAt: 1,
                updatedAt: 2,
              },
            ],
          },
        ],
      });
      useWorkbenchStore.setState({
        tabs: [
          ...tabs,
          {
            id: "browser-1",
            kind: "browser",
            title: "Example Domain",
            position: 2,
            pinned: false,
            dirty: false,
          },
        ],
      });
    });
    renderHeader(null);
    fireEvent.click(
      screen.getByRole("button", { name: "Show chat resources" }),
    );
    expect(
      within(screen.getByRole("region", { name: "Sources" })).queryByText("1"),
    ).toBeNull();
    expect(
      screen.getByTitle(
        "example.com - Recorded source; not a live browser link",
      ),
    ).toBeVisible();
    expect(
      screen.queryByRole("button", { name: "Open source example.com" }),
    ).toBeNull();
    expect(useWorkbenchStore.getState().activeTabId).not.toBe("browser-1");
    expect(screen.queryByText(/private/)).toBeNull();
  });

  it("excludes failed ACP tools and interrupted Codex children from working and done", () => {
    seedHeaderItems([
      collaborationItem(
        "spawn-1",
        0,
        "spawnAgent",
        ["child-1"],
        [["child-1", "running"]],
      ),
      collaborationItem("interrupt-1", 1, "interrupted", [], [], "child-1"),
      subagentTool("tool-pending", 2, "pending"),
      subagentTool("tool-completed", 3, "completed"),
      subagentTool("tool-failed", 4, "failed"),
      subagentTool("tool-declined", 5, "declined"),
      subagentTool("tool-cancelled", 6, "cancelled"),
    ]);
    renderHeader(null);
    fireEvent.click(
      screen.getByRole("button", { name: "Show chat resources" }),
    );
    expect(
      screen.getByLabelText("Recent subagents working: 1"),
    ).toBeInTheDocument();
    expect(
      screen.getByLabelText("Recent subagents done: 1"),
    ).toBeInTheDocument();
  });

  it("does not count a child whose spawn is outside the recent turn window", () => {
    seedHeaderItems([
      collaborationItem(
        "spawn-old",
        0,
        "spawnAgent",
        ["child-old"],
        [["child-old", "running"]],
      ),
    ]);
    const store = acpProjectionStoreFor("chat:a");
    const { scope, page } = store.getState();
    if (!scope || !page) throw new Error("Chat projection was absent.");
    const firstTurn = page.turns[0];
    const turns = Array.from({ length: 129 }, (_, index): TurnProjection => ({
      ...firstTurn,
      id: `turn-${index}` as TurnProjection["id"],
      ordinal: index,
      items:
        index === 0
          ? firstTurn.items
          : index === 128
            ? [
                collaborationItem(
                  "started-late",
                  1,
                  "started",
                  [],
                  [],
                  "child-old",
                ),
              ]
            : [],
    }));
    act(() => {
      store.getState().applyAuthoritativePage(scope, {
        ...page,
        projectionRevision: "3" as ProjectionRevision,
        turns,
      });
    });
    renderHeader(null);
    fireEvent.click(
      screen.getByRole("button", { name: "Show chat resources" }),
    );
    expect(screen.queryByRole("region", { name: "Subagents" })).toBeNull();
  });

  it("does not revive an interrupted child without another spawn", () => {
    seedHeaderItems([
      collaborationItem(
        "spawn-1",
        0,
        "spawnAgent",
        ["child-1"],
        [["child-1", "running"]],
      ),
      collaborationItem("interrupt-1", 1, "interrupted", [], [], "child-1"),
      collaborationItem("started-1", 2, "started", [], [], "child-1"),
    ]);
    renderHeader(null);
    fireEvent.click(
      screen.getByRole("button", { name: "Show chat resources" }),
    );
    expect(
      screen.getByLabelText("Recent subagents working: 0"),
    ).toBeInTheDocument();
    expect(
      screen.getByLabelText("Recent subagents done: 0"),
    ).toBeInTheDocument();
  });
  const headerTab: WorkbenchTab = {
    id: "chat:a",
    kind: "chat",
    title: "Agent View redesign",
    position: 0,
    pinned: false,
    dirty: false,
  };

  function renderHeader(
    snapshot: Parameters<typeof AgentTaskHeader>[0]["snapshot"],
  ) {
    act(() => {
      useWorkbenchStore.setState({
        project: { id: "project-a", name: "Sage", roots: [] },
        projects: [{ id: "project-a", name: "Sage", roots: [] }],
        projectTransitioning: false,
        projectGeneration: 7,
      } as never);
    });
    render(
      <AgentTaskHeader tab={headerTab} chatCount={2} snapshot={snapshot} />,
    );
  }

  it("spins a glyph beside the label while a turn is working", () => {
    renderHeader({
      chatId: "chat:a",
      statusLabel: "Running",
      statusTone: "working",
      latestText: "",
      streaming: true,
      waiting: false,
      working: true,
    });

    const status = screen.getByRole("status", { name: "Chat status: Running" });

    expect(status).toHaveTextContent("Running");
    expect(status.querySelector(".agent-chat-chip__spinner")).not.toBeNull();
  });

  it("leaves the idle status without a spinner", () => {
    renderHeader(null);

    const status = screen.getByRole("status", { name: "Chat status: Idle" });

    expect(status).toHaveTextContent("Idle");
    expect(status.querySelector(".agent-chat-chip__spinner")).toBeNull();
  });
});

describe("AgentChatPanes temporary side chat routing", () => {
  const sideTabs: WorkbenchTab[] = [
    ...tabs,
    {
      id: "side-x",
      kind: "chat",
      title: "Chat: Side chat",
      position: 2,
      pinned: false,
      dirty: false,
    },
  ];
  beforeEach(() => {
    useWorkbenchStore.setState({
      tabs: sideTabs,
      activeTabId: "side-x",
      project: { id: "project-a", name: "A" } as never,
    });
    useSideChatState.setState({
      sides: [{ id: "side-x", projectId: "project-a", parentChatId: "chat:a" }],
      split: SIDE_CHAT_SPLIT,
      pairedId: "side-x",
    });
  });
  afterEach(() =>
    useSideChatState.setState({
      sides: [],
      pendingClose: null,
      split: SIDE_CHAT_SPLIT,
      pairedId: null,
    }),
  );

  it("pairs the parent left and the side right at an unpersisted 43 / 57 split", async () => {
    render(<AgentChatPanes />);
    reportCenterWidth(1_000);
    expect(
      screen.getAllByTestId(/chat-pane-/).map((pane) => pane.dataset.testid),
    ).toEqual(["chat-pane-chat:a", "chat-pane-side-x"]);
    const splitter = await screen.findByRole("separator", {
      name: "Resize chat panes",
    });
    expect(splitter).toHaveAttribute("aria-valuenow", "43");
    fireEvent.keyDown(splitter, { key: "ArrowRight" });
    expect(splitter).toHaveAttribute("aria-valuenow", "48");
    fireEvent.keyDown(splitter, { key: "ArrowLeft", shiftKey: true });
    expect(splitter).toHaveAttribute("aria-valuenow", "38");
    fireEvent.keyDown(splitter, { key: "Home" });
    expect(splitter).toHaveAttribute("aria-valuenow", "30");
    fireEvent.keyDown(splitter, { key: "ArrowLeft" });
    expect(splitter).toHaveAttribute("aria-valuenow", "30");
    fireEvent.keyDown(splitter, { key: "End" });
    expect(splitter).toHaveAttribute("aria-valuenow", "70");
    fireEvent.keyDown(splitter, { key: "ArrowRight" });
    expect(splitter).toHaveAttribute("aria-valuenow", "70");
    expect(
      localStorage.getItem(WORKBENCH_LAYOUT_STORAGE_KEY) ?? "",
    ).not.toContain("48");
  });

  it("restores focus inside the explicit pair after an Editor resource roundtrip", () => {
    useSideChatState.setState({
      sides: [{ id: "side-x", projectId: "project-a", parentChatId: "chat:b" }],
    });
    const view = render(<AgentChatPanes />);
    view.unmount();
    useWorkbenchStore.setState({ activeTabId: "file:editor" });
    render(<AgentChatPanes />);
    expect(
      screen.getAllByTestId(/chat-pane-/).map((pane) => pane.dataset.testid),
    ).toEqual(["chat-pane-chat:b", "chat-pane-side-x"]);
    expect(pinnedAgentChatOwner()).toBe("chat:b");
  });

  it("keeps New chat available with a solo side pane and hidden navigation", async () => {
    const createChat = vi.fn(async () => undefined);
    useWorkbenchStore.setState({ createChat });
    render(<AgentChatPanes navigationCollapsed />);
    reportCenterWidth(859);
    await settleLazyModules();
    fireEvent.click(await screen.findByRole("button", { name: "New chat" }));
    expect(createChat).toHaveBeenCalledOnce();
  });

  it("keeps the parent's own header and puts the side tab in the same row (Codex s2/f0244)", async () => {
    const { container } = render(<AgentChatPanes />);
    reportCenterWidth(1_000);
    const header = container.querySelector(".agent-chat-center__header")!;
    expect(header).toHaveClass(
      "agent-chat-center__header--panes",
      "agent-chat-center__header--split",
    );
    expect(header).toHaveStyle({
      gridTemplateColumns: "minmax(0, 43fr) minmax(0, 57fr)",
    });
    await within(header as HTMLElement).findByText("Chat A");
    const cells = Array.from(header.children);
    expect(cells[0]).toHaveTextContent("Chat A");
    expect(cells[0]).not.toHaveTextContent("Side chat");
    expect(cells[1]).toHaveClass("agent-chat-center__side-tab");
    // Tight: the side chat shows alone with only its own tab in the row.
    reportCenterWidth(600);
    expect(screen.getAllByTestId(/chat-pane-/)).toHaveLength(1);
    expect(header).not.toHaveClass("agent-chat-center__header--split");
    expect(Array.from(header.children).map((cell) => cell.className)).toEqual([
      "agent-chat-center__side-tab",
    ]);
  });

  it("replaces the center when another Chat takes focus", async () => {
    render(<AgentChatPanes />);
    reportCenterWidth(1_000);
    await act(async () => {
      await useWorkbenchStore.getState().activateTab("chat:b");
    });
    expect(screen.getAllByTestId(/chat-pane-/)).toHaveLength(1);
    expect(screen.getByTestId("chat-pane-chat:b")).toBeInTheDocument();
    expect(screen.queryByRole("separator")).toBeNull();
  });

  it("shows a just-opened fork alone until the user selects another Chat (P3)", async () => {
    useSideChatState.setState({ sides: [] });
    await act(async () => {
      await useWorkbenchStore.getState().activateTab("chat:a");
    });
    render(<AgentChatPanes />);
    reportCenterWidth(1_000);
    expect(screen.getAllByTestId(/chat-pane-/)).toHaveLength(1);
    // The fork commit activates the fork.
    await act(async () => {
      await useWorkbenchStore.getState().activateTab("chat:b");
    });
    expect(
      screen.getAllByTestId(/chat-pane-/).map((pane) => pane.dataset.testid),
    ).toEqual(["chat-pane-chat:b"]);
    expect(screen.queryByRole("separator")).toBeNull();
    await act(async () => {
      await useWorkbenchStore.getState().activateTab("chat:a");
    });
    expect(screen.getAllByTestId(/chat-pane-/)).toHaveLength(1);
  });

  it.each(["chat:a", "side-x"])(
    "only reopens a responsive-hidden side pane through an explicit launch from %s",
    async (owner) => {
      render(<AgentChatPanes />);
      reportCenterWidth(1000);
      expect(screen.getAllByTestId(/chat-pane-/)).toHaveLength(2);
      reportCenterWidth(859);
      expect(screen.getAllByTestId(/chat-pane-/)).toHaveLength(1);
      for (const width of [860, 1200, 600, 1400]) {
        reportCenterWidth(width);
        expect(
          screen
            .getAllByTestId(/chat-pane-/)
            .map((pane) => pane.dataset.testid),
        ).toEqual(["chat-pane-side-x"]);
      }
      await act(async () => {
        await useWorkbenchStore.getState().activateTab(owner);
      });
      await act(async () => {
        await launchSideChat(
          owner,
          "project-a",
          useWorkbenchStore.getState().projectGeneration,
        );
      });
      expect(screen.getAllByTestId(/chat-pane-/)).toHaveLength(2);
      expect(screen.getByTestId("chat-pane-chat:a")).toBeInTheDocument();
      expect(screen.getByTestId("chat-pane-side-x")).toBeInTheDocument();
      expect(useSideChatState.getState().sides).toHaveLength(1);
    },
  );
});
