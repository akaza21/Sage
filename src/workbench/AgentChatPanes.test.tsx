import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { clearAcpProjectionRegistry } from "../app/acpProjectionRegistry";
import { acpProjectionStoreFor } from "../app/acpProjectionRegistry";
import { pinnedAgentChatOwner } from "../app/agentChatOwner";
import { useWorkbenchStore } from "../app/store";
import type { AcpChatPage, ProjectionRevision } from "../domain/acp";
import type { WorkbenchTab } from "../domain/workbench";
import { AgentChatPanes } from "./AgentChatPanes";
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

vi.mock("../features/chat/ChatSurface", () => ({
  ChatSurface: () => <div>Empty chat</div>,
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
  it("keeps two chats wide, shows the focused chat when tight, and restores the split", async () => {
    render(<AgentChatPanes />);

    reportCenterWidth(1_000);
    expect(screen.getAllByTestId(/chat-pane-/)).toHaveLength(2);
    const splitter = screen.getByRole("separator", {
      name: "Resize chat panes",
    });
    fireEvent.keyDown(splitter, { key: "ArrowRight" });
    expect(splitter).toHaveAttribute("aria-valuenow", "55");

    reportCenterWidth(859);
    expect(screen.getAllByTestId(/chat-pane-/)).toHaveLength(1);
    expect(screen.getByTestId("chat-pane-chat:b")).toBeInTheDocument();
    expect(screen.queryByRole("separator")).toBeNull();

    await act(async () => {
      await useWorkbenchStore.getState().activateTab("chat:a");
    });
    expect(screen.getByTestId("chat-pane-chat:a")).toBeInTheDocument();
    expect(screen.queryByTestId("chat-pane-chat:b")).toBeNull();

    reportCenterWidth(1_000);
    expect(screen.getAllByTestId(/chat-pane-/)).toHaveLength(2);
    expect(screen.getByRole("separator")).toHaveAttribute(
      "aria-valuenow",
      "55",
    );
  });

  it("disconnects its center-width observer on unmount", () => {
    const view = render(<AgentChatPanes />);
    view.unmount();
    expect(disconnect).toHaveBeenCalledTimes(1);
  });

  it("retains both visible Chat browse sessions across a focused page update", () => {
    useWorkbenchStore.setState({
      project: { id: "project-a", name: "A", roots: [] },
      projectGeneration: 7,
      nativeTransitionEpoch: 7,
      projectTransitioning: false,
    });
    seedPreviewOwner("chat:a", "session-a");
    seedPreviewOwner("chat:b", "session-b");
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
      chatId: "chat:b" as HarnessBrowseOwner["chatId"],
      sessionId: "session-b" as HarnessBrowseOwner["sessionId"],
    };
    render(<AgentChatPanes />);
    expect(harnessBrowseOwner()).toEqual(ownerB);
    setHarnessBrowseOwner(ownerA, ["chat:a", "chat:b"]);
    applyBrowseIntent({ owner: ownerA, url: "http://127.0.0.1:4173/a" });
    setHarnessBrowseOwner(ownerB, ["chat:a", "chat:b"]);
    applyBrowseIntent({ owner: ownerB, url: "http://127.0.0.1:4173/b" });

    const focused = acpProjectionStoreFor("chat:b");
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
