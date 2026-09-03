import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { clearAcpProjectionRegistry } from "../app/acpProjectionRegistry";
import { pinnedAgentChatOwner } from "../app/agentChatOwner";
import { useWorkbenchStore } from "../app/store";
import type { WorkbenchTab } from "../domain/workbench";
import { AgentChatPanes } from "./AgentChatPanes";

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

beforeEach(() => {
  localStorage.clear();
  clearAcpProjectionRegistry();
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
