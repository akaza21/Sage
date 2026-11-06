import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAgentChangesState } from "./agentChangesState";

const previewBridge = vi.hoisted(() => ({
  discover: vi.fn(),
  health: vi.fn(),
  start: vi.fn(),
  stop: vi.fn(),
  subscribe: vi.fn(),
  available: vi.fn(),
}));

const childBridge = vi.hoisted(() => ({ read: vi.fn() }));

vi.mock("../services/acpChildTranscript", () => ({
  getAcpChildTranscript: childBridge.read,
  subscribeChildProjectionChanges: async () => () => undefined,
}));

vi.mock("../features/preview/previewDesktop", () => ({
  discoverPreviewTargets: previewBridge.discover,
  getPreviewHealth: previewBridge.health,
  previewDesktopAvailable: previewBridge.available,
  startPreviewTarget: previewBridge.start,
  stopPreviewTarget: previewBridge.stop,
}));

const nativeBrowser = vi.hoisted(() => ({
  open: vi.fn(),
  control: vi.fn(),
  popupPending: vi.fn(),
  popupOpen: vi.fn(),
  popupAck: vi.fn(),
}));
const browserEvents = vi.hoisted(() => ({ listen: vi.fn() }));

vi.mock("@tauri-apps/api/event", () => browserEvents);

vi.mock("../ports/nativeBrowserPort", async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  openBrowser: nativeBrowser.open,
  browserAgentControl: nativeBrowser.control,
  browserPopupPending: nativeBrowser.popupPending,
  browserOpenPopup: nativeBrowser.popupOpen,
  browserPopupAck: nativeBrowser.popupAck,
  closeBrowser: async () => undefined,
}));

vi.mock("../services/acpDesktop", () => ({
  acpDesktop: { subscribePreviewProjectionChanges: previewBridge.subscribe },
}));

import type { ComponentProps } from "react";
import {
  acpProjectionStoreFor,
  clearAcpProjectionRegistry,
  pinAcpProjectionIds,
} from "../app/acpProjectionRegistry";
import { useAgentArtifactAdmissionStore } from "../app/agentArtifactAdmissionStore";
import {
  agentArtifactFollowRecordFor,
  useAgentArtifactFollowStore,
} from "../app/agentArtifactFollowStore";
import { clearAgentChatOwner, pinAgentChatOwner } from "../app/agentChatOwner";
import { createBrowserSlice } from "../app/browserSlice";
import { useWorkbenchStore } from "../app/store";
import { useSubagentPaneStore } from "../app/subagentPaneStore";
import type {
  AcpChatPage,
  AcpItem,
  ProjectGeneration,
  SageChatId,
  SageProjectId,
} from "../domain/acp";
import type { AgentArtifactOwner } from "../domain/agentArtifact";
import type { PreviewProjectionChangedEvent } from "../domain/previewProjection";
import { CollaborationItem } from "../features/chat/acp/CollaborationItem";
import {
  applyBrowseIntent,
  clearBrowseSession,
  clearBrowseSessions,
  setHarnessBrowseOwner,
  type HarnessBrowseOwner,
} from "../features/preview/harnessBrowse";
import { usePreviewTargetLifetime } from "../features/preview/usePreviewTargetLifetime";
import { settleLazyModules } from "../test/settleLazyModules";
import { AgentArtifactPane as Pane } from "./AgentArtifactPane";
function AgentArtifactPane(props: ComponentProps<typeof Pane>) {
  const publish = usePreviewTargetLifetime();
  return <Pane {...props} onPreviewTargetChange={publish} />;
}

function mountArtifactHeader() {
  return render(<div />).container.firstElementChild as HTMLElement;
}

it("disables browser creation at capacity and explains it inside the pane", async () => {
  vi.stubGlobal("__TAURI_INTERNALS__", {});
  useWorkbenchStore.setState({
    tabs: Array.from({ length: 8 }, (_, position) => ({
      id: `browser-${position}`,
      kind: "browser",
      title: `Page ${position}`,
      position,
      pinned: false,
      dirty: false,
    })),
  });
  render(<AgentArtifactPane />);
  await settleLazyModules();
  expect(
    screen.getByRole("button", { name: "New browser tab" }),
  ).toBeDisabled();
  expect(
    within(screen.getByLabelText("Agent artifacts")).getByRole("status"),
  ).toHaveTextContent("Browser tab limit reached");
  expect(useWorkbenchStore.getState().error).toBeNull();
  act(() =>
    useWorkbenchStore.setState((state) => ({ tabs: state.tabs.slice(1) })),
  );
  expect(screen.getByRole("button", { name: "New browser tab" })).toBeEnabled();
  expect(screen.queryByText(/Browser tab limit reached/)).toBeNull();
});

const initialState = useWorkbenchStore.getState();
const focusedOwner = {
  projectId: "project-a",
  chatId: "chat-focused",
  sessionId: "session-a",
} as AgentArtifactOwner;
const previewOwner = {
  projectId: "project-a",
  projectGeneration: "0",
  transitionEpoch: 0,
  chatId: "chat-focused",
  sessionId: "session-a",
  runtimeEpoch: "3",
} as HarnessBrowseOwner;

function admit(tabId: string): void {
  useAgentArtifactAdmissionStore.getState().admit({
    owner: focusedOwner,
    tabId,
  });
}

function openPreviewSession(): void {
  applyBrowseIntent({
    owner: previewOwner,
    url: "http://127.0.0.1:5173/native",
  });
}

function pageWithEdit(): AcpChatPage {
  const item = {
    id: "item-a",
    projectId: "project-a",
    chatId: "chat-focused",
    sessionId: "session-a",
    turnId: "turn-a",
    runtimeEpoch: "3",
    ordinal: 1,
    origin: "primary",
    activityId: null,
    lifecycle: "completed",
    revision: "1",
    createdAt: 1,
    updatedAt: 1,
    terminalAt: 1,
    kind: "fileChangeDiff",
    toolState: null,
    payload: {
      changes: [
        {
          resource: {
            kind: "resource",
            resource: { rootId: "root-a", relativePath: "src/app.ts" },
          },
          operation: "updated",
          originalResource: null,
          summary: "edited",
          additions: 3,
          deletions: 1,
        },
      ],
      truncated: false,
    },
  } as unknown as AcpItem;
  return {
    projectId: "project-a",
    chatId: "chat-focused",
    title: "Focused",
    projectionRevision: "8",
    currentSessionId: "session-a",
    sessions: [{ id: "session-a", ordinal: 1, runtimeEpoch: "3" }],
    turns: [
      {
        id: "turn-a",
        sessionId: "session-a",
        runtimeEpoch: "3",
        ordinal: 2,
        state: "completed",
        revision: "1",
        createdAt: 1,
        updatedAt: 2,
        terminalAt: 2,
        items: [item],
        activities: [],
      },
    ],
    nextCursor: null,
  } as unknown as AcpChatPage;
}

function seedFocusedPage(): void {
  pinAcpProjectionIds(["chat-focused"]);
  const store = acpProjectionStoreFor("chat-focused");
  const scope = {
    projectId: "project-a" as SageProjectId,
    projectGeneration: "0" as ProjectGeneration,
    chatId: "chat-focused" as SageChatId,
  };
  store.getState().activateScope(scope);
  expect(store.getState().applyAuthoritativePage(scope, pageWithEdit())).toBe(
    "accepted",
  );
}

beforeEach(() => {
  nativeBrowser.open.mockReset();
  nativeBrowser.control.mockReset().mockResolvedValue(null);
  nativeBrowser.popupPending.mockReset().mockResolvedValue([]);
  nativeBrowser.popupOpen.mockReset();
  nativeBrowser.popupAck.mockReset().mockResolvedValue(undefined);
  childBridge.read.mockReset();
  childBridge.read.mockResolvedValue({
    kind: "current",
    retentionLimited: false,
    turns: [],
  });
  previewBridge.discover.mockReset();
  previewBridge.health.mockReset();
  previewBridge.start.mockReset();
  previewBridge.stop.mockReset();
  previewBridge.subscribe.mockReset();
  previewBridge.available.mockReset();
  previewBridge.discover.mockResolvedValue({ targets: [], truncated: false });
  previewBridge.health.mockResolvedValue({ healthy: true, origin: null });
  previewBridge.start.mockResolvedValue(undefined);
  previewBridge.stop.mockResolvedValue(undefined);
  previewBridge.subscribe.mockResolvedValue(vi.fn());
  previewBridge.available.mockReturnValue(true);
  useAgentArtifactFollowStore.getState().clear();
  useSubagentPaneStore.setState({ open: null, selected: false, list: false });
  seedFocusedPage();
  pinAgentChatOwner(focusedOwner.chatId);
  setHarnessBrowseOwner(previewOwner, ["chat-focused", "chat-second"]);
  useWorkbenchStore.setState({
    project: { id: "project-a", name: "Project A", roots: [] },
    projectGeneration: 0,
    projectTransitioning: false,
    tabs: [
      {
        id: "chat-focused",
        kind: "chat",
        title: "Focused",
        position: 0,
        pinned: false,
        dirty: false,
      },
      {
        id: "file:app",
        kind: "file",
        title: "app.ts",
        position: 1,
        pinned: false,
        dirty: false,
        resource: { rootId: "root-a", relativePath: "src/app.ts" },
      },
      {
        id: "diff:other",
        kind: "diff",
        title: "other.ts",
        position: 2,
        pinned: false,
        dirty: false,
        resource: { rootId: "root-a", relativePath: "src/other.ts" },
      },
    ],
    activeTabId: "chat-focused",
  });
});

afterEach(() => {
  window.dispatchEvent(new Event("beforeunload"));
  cleanup();
  clearBrowseSessions();
  useAgentChangesState.getState().close();
  clearAcpProjectionRegistry();
  clearAgentChatOwner();
  useAgentArtifactAdmissionStore.getState().clear();
  useAgentArtifactFollowStore.getState().clear();
  useSubagentPaneStore.setState({ open: null, selected: false, list: false });
  vi.unstubAllGlobals();
  useWorkbenchStore.setState(initialState, true);
});

describe("Agent artifact file chips", () => {
  it("unmounts Changes when selecting a normal file tab", async () => {
    admit("file:app");
    render(<AgentArtifactPane expanded onExpandedChange={vi.fn()} />);
    act(() => useAgentChangesState.getState().show(focusedOwner, 0));
    expect(await screen.findByRole("tab", { name: /Changes/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(
      await screen.findByRole("region", { name: "Chat changes" }),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("tab", { name: /app.ts/ }));
    expect(useAgentChangesState.getState().selected).toBe(false);
    expect(screen.queryByRole("region", { name: "Chat changes" })).toBeNull();
  });
  it.each([
    "dismissal",
    "Chat focus round-trip",
    "project-transition round-trip",
  ])(
    "keeps an explicitly selected child collapsed after %s until another explicit open",
    (transition) => {
      const onExpandedChange = vi.fn();
      const base = pageWithEdit();
      const child = {
        ...base.turns[0].items[0],
        id: "selected-child",
        kind: "collaboration",
        payload: {
          activityId: null,
          label: "spawnAgent",
          summary: "",
          receiverLabels: ["Reviewer"],
          operation: "spawnAgent",
          agentKeys: ["child-a"],
          agentStates: [["child-a", "running"]],
        },
      } as unknown as AcpItem;
      const store = acpProjectionStoreFor("chat-focused");
      const scope = store.getState().scope;
      if (!scope) throw new Error("Missing focused scope");
      store.getState().applyAuthoritativePage(scope, {
        ...base,
        turns: [{ ...base.turns[0], items: [child] }],
      });
      const target = {
        owner: focusedOwner,
        itemId: child.id,
        agentKey: "child-a",
      };
      useSubagentPaneStore.getState().show(target);
      const view = render(
        <AgentArtifactPane expanded onExpandedChange={onExpandedChange} />,
      );
      view.rerender(
        <AgentArtifactPane
          expanded={false}
          headerNode={mountArtifactHeader()}
          onExpandedChange={onExpandedChange}
        />,
      );
      expect(onExpandedChange).not.toHaveBeenCalled();
      if (transition === "Chat focus round-trip") {
        act(() => pinAgentChatOwner("chat-second"));
        expect(onExpandedChange).not.toHaveBeenCalled();
        act(() => pinAgentChatOwner(focusedOwner.chatId));
      } else if (transition === "project-transition round-trip") {
        act(() => useWorkbenchStore.setState({ projectTransitioning: true }));
        expect(onExpandedChange).not.toHaveBeenCalled();
        act(() => useWorkbenchStore.setState({ projectTransitioning: false }));
      }
      expect(onExpandedChange).not.toHaveBeenCalled();
      act(() =>
        store.getState().applyAuthoritativePage(scope, {
          ...base,
          turns: [
            {
              ...base.turns[0],
              items: [child, { ...child, id: "passive-new" } as AcpItem],
            },
          ],
        }),
      );
      expect(onExpandedChange).not.toHaveBeenCalled();
      expect(screen.queryByRole("tab", { name: "Subagents" })).toBeNull();
      act(() => useSubagentPaneStore.getState().show(target));
      expect(onExpandedChange).toHaveBeenCalledWith(true);
    },
  );
  it("counts distinct grouped subagents in the collapsed header, not the single Subagents tab", () => {
    const base = pageWithEdit();
    const child = {
      ...base.turns[0].items[0],
      id: "grouped",
      kind: "collaboration",
      payload: {
        label: "spawnAgent",
        summary: "",
        activityId: null,
        operation: "spawnAgent",
        agentKeys: ["child-a", "child-b"],
        receiverLabels: ["Reviewer", "Builder"],
        agentStates: [
          ["child-a", "completed"],
          ["child-b", "completed"],
        ],
      },
    } as unknown as AcpItem;
    const native = {
      ...child,
      id: "native",
      payload: {
        ...child.payload,
        label: "Reviewer",
        operation: "completed",
        agentKey: "child-a",
        agentKeys: [],
        receiverLabels: [],
        agentStates: [],
      },
    } as unknown as AcpItem;
    const store = acpProjectionStoreFor("chat-focused");
    const scope = store.getState().scope!;
    store.getState().applyAuthoritativePage(scope, {
      ...base,
      turns: [
        { ...base.turns[0], items: [child, native] },
        {
          ...base.turns[0],
          id: "later-turn",
          ordinal: 3,
          items: [],
        } as unknown as AcpChatPage["turns"][number],
      ],
    });
    render(
      <AgentArtifactPane expanded={false} headerNode={mountArtifactHeader()} />,
    );
    expect(
      screen.getByRole("button", {
        name: "Show files and preview \u00b7 Ready \u00b7 2 resources",
      }),
    ).toBeVisible();
  });

  it("keeps passive child arrivals collapsed and expands only for explicit selection", async () => {
    const onExpandedChange = vi.fn();
    const base = pageWithEdit();
    const child = {
      ...base.turns[0].items[0],
      id: "passive-child",
      kind: "collaboration",
      payload: {
        activityId: null,
        label: "spawnAgent",
        summary: "",
        receiverLabels: ["Reviewer"],
        operation: "spawnAgent",
        agentKeys: ["child-a"],
        agentStates: [["child-a", "running"]],
      },
    } as unknown as AcpItem;
    const store = acpProjectionStoreFor("chat-focused");
    const scope = store.getState().scope;
    if (!scope) throw new Error("Missing focused scope");
    store.getState().applyAuthoritativePage(scope, {
      ...base,
      turns: [{ ...base.turns[0], items: [] }],
    });
    const view = render(
      <AgentArtifactPane
        expanded={false}
        headerNode={mountArtifactHeader()}
        onExpandedChange={onExpandedChange}
      />,
    );
    act(() => {
      store.getState().applyAuthoritativePage(scope, {
        ...base,
        turns: [{ ...base.turns[0], items: [child] }],
      });
    });
    expect(onExpandedChange).not.toHaveBeenCalled();
    expect(screen.queryByRole("tab", { name: "Subagents" })).toBeNull();
    expect(screen.queryByRole("heading", { name: "Reviewer" })).toBeNull();
    act(() => {
      useSubagentPaneStore.getState().show({
        owner: focusedOwner,
        itemId: child.id,
        agentKey: "child-a",
      });
    });
    expect(onExpandedChange).toHaveBeenCalledWith(true);
    view.rerender(
      <AgentArtifactPane expanded onExpandedChange={onExpandedChange} />,
    );
    expect(screen.getByRole("tab", { name: "Subagents" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(
      await screen.findByRole("heading", { name: "Reviewer" }),
    ).toBeVisible();
  });
  it("reads a child with the live native epoch when projection generation has drifted", async () => {
    const base = pageWithEdit();
    const child = {
      ...base.turns[0].items[0],
      id: "native-child",
      kind: "collaboration",
      payload: {
        activityId: null,
        label: "Reviewer",
        summary: "",
        receiverLabels: ["Reviewer"],
        operation: "started",
        agentKey: "child-a",
        agentKeys: ["child-a"],
        agentStates: [["child-a", "running"]],
      },
    } as unknown as AcpItem;
    useWorkbenchStore.setState({
      projectGeneration: 3,
      nativeTransitionEpoch: 2,
    });
    const store = acpProjectionStoreFor("chat-focused");
    const scope = {
      projectId: "project-a" as SageProjectId,
      projectGeneration: "3" as ProjectGeneration,
      chatId: "chat-focused" as SageChatId,
    };
    store.getState().activateScope(scope);
    expect(
      store.getState().applyAuthoritativePage(scope, {
        ...base,
        sessions: [
          {
            ...base.sessions[0],
            capabilities: {
              ...base.sessions[0].capabilities,
              childTranscriptRead: true,
            },
          },
        ],
        turns: [{ ...base.turns[0], items: [child] }],
      } as AcpChatPage),
    ).toBe("accepted");
    useSubagentPaneStore.getState().show({
      owner: focusedOwner,
      itemId: child.id,
      agentKey: "child-a",
    });

    render(<AgentArtifactPane expanded />);

    await waitFor(() =>
      expect(childBridge.read).toHaveBeenCalledWith({
        projectId: "project-a",
        projectGeneration: "2",
        chatId: "chat-focused",
        sessionId: "session-a",
        runtimeEpoch: "3",
        itemId: "native-child",
        agentKey: "child-a",
      }),
    );
    expect(store.getState().scope?.projectGeneration).toBe("3");
    act(() => useWorkbenchStore.setState({ nativeTransitionEpoch: 4 }));
    await waitFor(() =>
      expect(childBridge.read).toHaveBeenLastCalledWith(
        expect.objectContaining({ projectGeneration: "4" }),
      ),
    );
  });
  it("does not expose Subagents for conflicting child identities", () => {
    const base = pageWithEdit();
    const child = {
      ...base.turns[0].items[0],
      id: "conflict-child",
      kind: "collaboration",
      payload: {
        activityId: null,
        label: "Reviewer",
        summary: "",
        receiverLabels: ["Reviewer"],
        operation: "started",
        agentKey: "child-a",
        agentKeys: ["child-b"],
        agentStates: [["child-b", "running"]],
      },
    } as unknown as AcpItem;
    const store = acpProjectionStoreFor("chat-focused");
    const scope = store.getState().scope;
    if (!scope) throw new Error("Missing focused scope");
    expect(
      store.getState().applyAuthoritativePage(scope, {
        ...base,
        turns: [{ ...base.turns[0], items: [child] }],
      } as AcpChatPage),
    ).toBe("accepted");
    render(<AgentArtifactPane />);
    expect(screen.queryByRole("tab", { name: "Subagents" })).toBeNull();
  });
  it("keeps Subagents available beside resources before a child row is opened", async () => {
    const base = pageWithEdit();
    const child = {
      ...base.turns[0].items[0],
      id: "spawn-child",
      kind: "collaboration",
      payload: {
        activityId: null,
        label: "spawnAgent",
        summary: "",
        receiverLabels: ["Reviewer"],
        operation: "spawnAgent",
        agentKeys: ["child-a"],
        agentStates: [["child-a", "running"]],
      },
    } as unknown as AcpItem;
    const store = acpProjectionStoreFor("chat-focused");
    const scope = store.getState().scope;
    if (!scope) throw new Error("Missing focused scope");
    expect(
      store.getState().applyAuthoritativePage(scope, {
        ...base,
        turns: [{ ...base.turns[0], items: [base.turns[0].items[0], child] }],
      } as AcpChatPage),
    ).toBe("accepted");
    admit("file:app");
    render(<AgentArtifactPane />);
    const tab = screen.getByRole("tab", { name: "Subagents" });
    expect(tab).toHaveAttribute("aria-selected", "false");
    // Codex s4/f0244: a person outline, not sparkles, marks the tab.
    expect(tab.querySelector(".tab__person")).not.toBeNull();
    expect(tab.querySelector(".lucide-sparkles")).toBeNull();
    fireEvent.click(tab);
    expect(tab).toHaveAttribute("aria-selected", "true");
    // The plural tab opens on its Active / Done list.
    const active = await screen.findByRole("region", {
      name: "Active subagents",
    });
    expect(active).toHaveTextContent("Active · 1");
    fireEvent.click(
      screen.getByRole("button", { name: "Open subagent Reviewer" }),
    );
    expect(
      await screen.findByRole("heading", { name: "Reviewer" }),
    ).toBeVisible();
    fireEvent.click(screen.getByRole("tab", { name: /File src\/app.ts/ }));
    expect(tab).toHaveAttribute("aria-selected", "false");
    expect(tab).toBeVisible();
  });
  it("navigates from Changes to a child selected by its transcript name", async () => {
    const base = pageWithEdit();
    const child = {
      ...base.turns[0].items[0],
      id: "spawn-child",
      kind: "collaboration",
      payload: {
        activityId: null,
        label: "spawnAgent",
        summary: "",
        receiverLabels: ["Reviewer"],
        operation: "spawnAgent",
        agentKeys: ["child-a"],
        agentStates: [["child-a", "running"]],
      },
    } as unknown as Extract<AcpItem, { kind: "collaboration" }>;
    const store = acpProjectionStoreFor("chat-focused");
    const scope = store.getState().scope;
    if (!scope) throw new Error("Missing scope");
    store.getState().applyAuthoritativePage(scope, {
      ...base,
      turns: [{ ...base.turns[0], items: [child] }],
    });
    render(
      <>
        <CollaborationItem item={child} />
        <AgentArtifactPane />
      </>,
    );
    act(() => useAgentChangesState.getState().show(focusedOwner, 0));
    expect(
      await screen.findByRole("region", { name: "Chat changes" }),
    ).toBeVisible();
    fireEvent.click(
      screen.getByRole("button", { name: "Open subagent Reviewer" }),
    );
    expect(screen.queryByRole("region", { name: "Chat changes" })).toBeNull();
    expect(
      await screen.findByRole("heading", { name: "Reviewer" }),
    ).toBeVisible();
  });
  it("opens a historical-session Subagent row in the focused Chat", async () => {
    const base = pageWithEdit();
    const older = {
      ...base.turns[0].items[0],
      id: "older-spawn",
      sessionId: "session-older",
      turnId: "turn-older",
      kind: "collaboration",
      payload: {
        activityId: null,
        label: "spawnAgent",
        summary: "",
        receiverLabels: ["Historian"],
        operation: "spawnAgent",
        agentKeys: ["hidden-child"],
        agentStates: [["hidden-child", "completed"]],
      },
    } as unknown as AcpItem;
    const page = {
      ...base,
      currentSessionId: null,
      turns: [
        {
          ...base.turns[0],
          id: "turn-older",
          sessionId: "session-older",
          items: [older],
        },
        ...base.turns,
      ],
    } as unknown as AcpChatPage;
    clearAcpProjectionRegistry();
    pinAcpProjectionIds(["chat-focused"]);
    const store = acpProjectionStoreFor("chat-focused");
    store.getState().activateScope({
      projectId: focusedOwner.projectId,
      projectGeneration: "0" as ProjectGeneration,
      chatId: focusedOwner.chatId,
    });
    expect(
      store.getState().applyAuthoritativePage(
        {
          projectId: focusedOwner.projectId,
          projectGeneration: "0" as ProjectGeneration,
          chatId: focusedOwner.chatId,
        },
        page,
      ),
    ).toBe("accepted");
    useSubagentPaneStore.getState().show({
      owner: {
        ...focusedOwner,
        sessionId: "session-older" as AgentArtifactOwner["sessionId"],
      },
      itemId: "older-spawn",
      agentKey: "hidden-child",
    });
    render(<AgentArtifactPane />);
    expect(screen.getByRole("tab", { name: "Subagents" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(
      await screen.findByRole("heading", { name: "Historian" }),
    ).toBeVisible();
    act(() => useWorkbenchStore.setState({ projectTransitioning: true }));
    expect(screen.queryByRole("tab", { name: "Subagents" })).toBeNull();
    expect(screen.queryByRole("heading", { name: "Historian" })).toBeNull();
  });
  it("starts empty with a collapsed header resource affordance", () => {
    const onExpandedChange = vi.fn();
    pinAgentChatOwner(null);
    useWorkbenchStore.setState({
      tabs: [
        {
          id: "chat-focused",
          kind: "chat",
          title: "Focused",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-focused",
    });

    const { rerender } = render(
      <AgentArtifactPane
        expanded={false}
        headerNode={mountArtifactHeader()}
        onExpandedChange={onExpandedChange}
      />,
    );

    const expand = screen.getByRole("button", {
      name: "Show files and preview · Waiting",
    });
    expect(expand).toBeVisible();
    expect(within(expand).queryByText("Files")).toBeNull();
    expect(within(expand).queryByText("Changes")).toBeNull();
    expect(within(expand).queryByText("Preview")).toBeNull();
    expect(
      screen.queryByRole("heading", { name: "Waiting for this Chat" }),
    ).toBeNull();

    fireEvent.click(expand);
    expect(onExpandedChange).toHaveBeenLastCalledWith(true);

    rerender(
      <AgentArtifactPane expanded onExpandedChange={onExpandedChange} />,
    );
    expect(
      screen.getByRole("heading", { name: "Waiting for this Chat" }),
    ).toBeVisible();
    expect(
      screen.getByText(/Open a file or change from the conversation/),
    ).toBeVisible();
    fireEvent.keyDown(screen.getByLabelText("Agent artifacts"), {
      key: "Escape",
    });
    expect(onExpandedChange).toHaveBeenLastCalledWith(false);

    rerender(
      <AgentArtifactPane
        expanded={false}
        headerNode={mountArtifactHeader()}
        onExpandedChange={onExpandedChange}
      />,
    );
    expect(
      screen.getByRole("button", {
        name: "Show files and preview · Waiting",
      }),
    ).toHaveFocus();
  });

  it("requests expansion when a new Agent file becomes available", () => {
    const onExpandedChange = vi.fn();
    useWorkbenchStore.setState({
      tabs: [
        {
          id: "chat-focused",
          kind: "chat",
          title: "Focused",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-focused",
    });
    render(
      <AgentArtifactPane
        expanded={false}
        headerNode={mountArtifactHeader()}
        onExpandedChange={onExpandedChange}
      />,
    );

    act(() => {
      useWorkbenchStore.setState((state) => ({
        tabs: [
          ...state.tabs,
          {
            id: "file:new",
            kind: "file",
            title: "new.ts",
            position: 1,
            pinned: false,
            dirty: false,
            resource: { rootId: "root-a", relativePath: "src/new.ts" },
          },
        ],
      }));
      admit("file:new");
    });

    expect(onExpandedChange).toHaveBeenCalledWith(true);
  });

  it("does not append Preview before it is open and keeps File kind in accessible metadata", () => {
    render(<AgentArtifactPane />);

    expect(screen.queryByRole("tab", { name: "Preview" })).toBeNull();
    const fileTab = screen.getByRole("tab", { name: "File src/app.ts" });
    expect(fileTab).toHaveAttribute("title", "File src/app.ts");
    expect(within(fileTab).queryByText("File")).toBeNull();
    expect(fileTab).toHaveAttribute("aria-selected", "true");
    expect(
      screen.queryByText(/Open a file or change from the conversation/),
    ).toBeNull();
    expect(screen.queryByTitle("Sage browser")).toBeNull();
  });

  it("shows +3 −1 on a matched file tab and hides an Editor-only unmatched tab", () => {
    render(<AgentArtifactPane />);

    const matched = screen.getByRole("tab", { name: /app\.ts/ });
    expect(matched).toHaveAttribute("aria-label", "File src/app.ts");
    expect(within(matched).getByText("+3")).toBeInTheDocument();
    expect(within(matched).getByText("-1")).toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: /other\.ts$/ })).toBeNull();

    expect(screen.queryByRole("tab", { name: "Preview" })).toBeNull();
  });

  it("renders a truthful artifact header and pins navigation through its ordered targets", () => {
    admit("file:app");
    openPreviewSession();
    render(<AgentArtifactPane />);
    fireEvent.click(screen.getByRole("tab", { name: "File src/app.ts" }));

    expect(screen.getByText("Opened by Sage")).toBeVisible();
    expect(screen.getByText("VIEW ONLY")).toBeVisible();
    expect(screen.getByText("src/app.ts")).toBeVisible();
    expect(screen.getByText("1 of 2")).toHaveAttribute("aria-live", "polite");
    expect(
      screen.getByRole("button", { name: "Previous artifact" }),
    ).toBeDisabled();

    fireEvent.click(screen.getByRole("button", { name: "Next artifact" }));
    expect(screen.queryByText("2 of 2")).toBeNull();
    expect(screen.queryByText("Opened by Sage")).toBeNull();
    expect(screen.queryByText("VIEW ONLY")).toBeNull();
    expect(screen.queryByRole("button", { name: "Copy path" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Open in Editor" })).toBeNull();
    expect(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        focusedOwner,
      ),
    ).toMatchObject({
      mode: "pinned",
      selected: { kind: "preview" },
    });
  });

  it("does not claim Sage opened an edit-summary-visible tab", () => {
    render(<AgentArtifactPane />);

    expect(screen.queryByText("Opened by Sage")).toBeNull();
  });

  it("copies the repository-relative path", async () => {
    openPreviewSession();
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", {
      ...navigator,
      clipboard: { writeText },
    });
    render(<AgentArtifactPane />);
    fireEvent.click(screen.getByRole("tab", { name: "File src/app.ts" }));

    fireEvent.click(screen.getByRole("button", { name: "Copy path" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("src/app.ts"));
    expect(
      screen.getByRole("button", { name: "Path copied" }),
    ).toHaveTextContent("Copied");
    fireEvent.click(screen.getByRole("button", { name: "Next artifact" }));
    expect(screen.queryByRole("button", { name: "Copy path" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Open in Editor" })).toBeNull();
    fireEvent.click(screen.getByRole("tab", { name: "File src/app.ts" }));
    expect(screen.getByRole("button", { name: "Copy path" })).toHaveTextContent(
      "Copy path",
    );
  });

  it("opens an Agent file in Editor from the File pane Open menu", async () => {
    const onOpenInEditor = vi.fn();
    admit("file:app");
    render(<AgentArtifactPane onOpenInEditor={onOpenInEditor} />);
    fireEvent.click(screen.getByRole("tab", { name: "File src/app.ts" }));

    fireEvent.click(screen.getByRole("button", { name: "Open choices" }));
    const menu = await screen.findByRole("menu", { name: "Open file" });
    fireEvent.click(
      within(menu).getByRole("menuitem", { name: "Open in Editor" }),
    );

    expect(onOpenInEditor).toHaveBeenCalledExactlyOnceWith("file:app");
    expect(screen.getByRole("button", { name: "Open choices" })).toHaveFocus();
  });

  it("returns focus after copying from the File pane Open menu", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { ...navigator, clipboard: { writeText } });
    render(<AgentArtifactPane />);
    fireEvent.click(screen.getByRole("tab", { name: "File src/app.ts" }));

    fireEvent.click(screen.getByRole("button", { name: "Open choices" }));
    fireEvent.click(
      within(await screen.findByRole("menu", { name: "Open file" })).getByRole(
        "menuitem",
        { name: "Copy path" },
      ),
    );

    await waitFor(() => expect(writeText).toHaveBeenCalledWith("src/app.ts"));
    expect(screen.getByRole("button", { name: "Open choices" })).toHaveFocus();
  });

  it("opens Agent Preview as a loopback browser without the app-script starter", async () => {
    applyBrowseIntent({
      owner: previewOwner,
      url: "http://127.0.0.1:5173/",
    });
    render(<AgentArtifactPane />);
    fireEvent.click(screen.getByRole("tab", { name: "Preview" }));
    await settleLazyModules();
    expect(screen.getByLabelText("Sage browser")).toBeInTheDocument();
    expect(screen.getByLabelText("Preview address")).toBeInTheDocument();
    expect(
      screen.queryByText("No dev or start scripts were found."),
    ).toBeNull();
    expect(screen.queryByRole("button", { name: "Refresh" })).toBeNull();
  });

  it("keeps the same Preview frame asleep across collapse and expand", async () => {
    applyBrowseIntent({
      owner: previewOwner,
      url: "http://127.0.0.1:5173/",
    });
    const view = render(<AgentArtifactPane />);
    fireEvent.click(screen.getByRole("tab", { name: "Preview" }));
    await settleLazyModules();
    const frame = screen.getByTitle("Sage browser");

    view.rerender(<AgentArtifactPane expanded={false} />);
    await settleLazyModules();
    expect(frame.isConnected).toBe(true);
    expect(frame).not.toBeVisible();

    view.rerender(<AgentArtifactPane expanded />);
    await settleLazyModules();
    expect(screen.getByTitle("Sage browser")).toBe(frame);
    expect(frame).toBeVisible();
  });

  it("keeps the collapsed header waiting without a Preview session", () => {
    const onExpandedChange = vi.fn();
    useWorkbenchStore.setState({
      tabs: [
        {
          id: "chat-focused",
          kind: "chat",
          title: "Focused",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-focused",
    });

    render(
      <AgentArtifactPane
        expanded={false}
        headerNode={mountArtifactHeader()}
        onExpandedChange={onExpandedChange}
      />,
    );

    expect(
      screen.getByRole("button", {
        name: "Show files and preview · Waiting",
      }),
    ).not.toHaveTextContent("1");
  });

  it("counts a live Preview on the collapsed header without File or Diff tabs", () => {
    const onExpandedChange = vi.fn();
    useWorkbenchStore.setState({
      tabs: [
        {
          id: "chat-focused",
          kind: "chat",
          title: "Focused",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-focused",
    });
    applyBrowseIntent({
      owner: previewOwner,
      url: "http://127.0.0.1:5173/preview",
    });

    render(
      <AgentArtifactPane
        expanded={false}
        headerNode={mountArtifactHeader()}
        onExpandedChange={onExpandedChange}
      />,
    );

    expect(
      screen.getByRole("button", {
        name: "Show files and preview · Ready · 1 resource",
      }),
    ).toHaveTextContent("1");
  });

  it("keeps a standalone manual browser quiet when native Preview is unavailable", async () => {
    previewBridge.available.mockReturnValue(false);
    applyBrowseIntent({
      owner: previewOwner,
      url: "http://127.0.0.1:5173/manual",
    });
    render(<AgentArtifactPane />);
    fireEvent.click(screen.getByRole("tab", { name: "Preview" }));
    await settleLazyModules();
    expect(screen.getByTitle("Sage browser")).toHaveAttribute(
      "src",
      "http://127.0.0.1:5173/manual",
    );
    expect(previewBridge.discover).not.toHaveBeenCalled();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("surfaces a desktop Preview subscription failure without replacing manual browsing", async () => {
    previewBridge.subscribe.mockRejectedValue(
      new Error("Preview listener registration failed"),
    );
    applyBrowseIntent({
      owner: previewOwner,
      url: "http://127.0.0.1:5173/manual",
    });
    render(<AgentArtifactPane />);
    fireEvent.click(screen.getByRole("tab", { name: "Preview" }));
    await settleLazyModules();
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(
        "Preview listener registration failed",
      ),
    );
    expect(screen.getByTitle("Sage browser")).toHaveAttribute(
      "src",
      "http://127.0.0.1:5173/manual",
    );
    expect(previewBridge.discover).not.toHaveBeenCalled();
  });

  it("clears a rejected discovery notice once native Preview reports an active origin", async () => {
    let receive: ((event: PreviewProjectionChangedEvent) => void) | null = null;
    previewBridge.subscribe.mockImplementation(async (handler) => {
      receive = handler;
      return vi.fn();
    });
    previewBridge.discover.mockRejectedValue(
      new Error("Preview request identity is invalid."),
    );
    const target = {
      projectId: "project-a",
      projectGeneration: "0",
      expectedTransitionEpoch: 0,
      chatId: "chat-focused",
      sessionId: "session-a",
      runtimeEpoch: "3",
      rootId: "root-a",
      scriptName: "dev",
    } as const;
    applyBrowseIntent({
      owner: previewOwner,
      url: "http://127.0.0.1:5173/manual",
    });
    render(<AgentArtifactPane />);
    fireEvent.click(screen.getByRole("tab", { name: "Preview" }));
    await settleLazyModules();
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(
        "Preview request identity is invalid.",
      ),
    );

    act(() =>
      receive?.({
        kind: "preview",
        preview: {
          ...previewOwner,
          previewRevision: "2",
          phase: "active",
          failure: null,
          target,
          origin: "http://127.0.0.1:5173/native",
        },
      } as PreviewProjectionChangedEvent),
    );

    await waitFor(() =>
      expect(screen.queryByText("Preview is unavailable")).toBeNull(),
    );
    expect(screen.getByTitle("Sage browser")).toHaveAttribute(
      "src",
      "http://127.0.0.1:5173/native",
    );
  });

  it("keeps the native controller through File, tab, and collapse sleep-resume", async () => {
    openPreviewSession();
    const unlisten = vi.fn();
    previewBridge.subscribe.mockResolvedValue(unlisten);
    const onExpandedChange = vi.fn();
    const view = render(
      <AgentArtifactPane expanded onExpandedChange={onExpandedChange} />,
    );

    expect(previewBridge.discover).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("tab", { name: "Preview" }));
    await settleLazyModules();
    await waitFor(() => expect(previewBridge.discover).toHaveBeenCalledOnce());
    fireEvent.click(screen.getByRole("tab", { name: "File src/app.ts" }));
    expect(screen.queryByLabelText("Sage browser")).toBeNull();
    fireEvent.click(screen.getByRole("tab", { name: "Preview" }));
    await settleLazyModules();
    expect(previewBridge.discover).toHaveBeenCalledOnce();
    expect(previewBridge.stop).not.toHaveBeenCalled();

    view.rerender(
      <AgentArtifactPane
        expanded={false}
        headerNode={mountArtifactHeader()}
        onExpandedChange={onExpandedChange}
      />,
    );
    view.rerender(
      <AgentArtifactPane expanded onExpandedChange={onExpandedChange} />,
    );
    await waitFor(() => expect(previewBridge.discover).toHaveBeenCalledOnce());
    expect(previewBridge.stop).not.toHaveBeenCalled();
    expect(unlisten).not.toHaveBeenCalled();
  });

  it("waits for the Agent listener before discovering and enabling Start", async () => {
    openPreviewSession();
    let ready: ((unlisten: () => void) => void) | null = null;
    const target = {
      projectId: "project-a",
      projectGeneration: "0",
      expectedTransitionEpoch: 0,
      chatId: "chat-focused",
      sessionId: "session-a",
      runtimeEpoch: "3",
      rootId: "root-a",
      scriptName: "dev",
    } as const;
    previewBridge.subscribe.mockImplementation(
      () =>
        new Promise<() => void>((resolve) => {
          ready = resolve;
        }),
    );
    previewBridge.discover.mockResolvedValue({ targets: [target] });
    render(<AgentArtifactPane />);
    fireEvent.click(screen.getByRole("tab", { name: "Preview" }));
    await settleLazyModules();
    await waitFor(() => expect(previewBridge.subscribe).toHaveBeenCalledOnce());
    expect(previewBridge.discover).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Start" })).toBeNull();

    if (!ready) throw new Error("listener readiness resolver was unavailable");
    (ready as (dispose: () => void) => void)(vi.fn());
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Start" })).toBeEnabled(),
    );
    expect(previewBridge.discover).toHaveBeenCalledOnce();
  });

  it("renders compact bounded Preview targets and one typed native failure notice", async () => {
    openPreviewSession();
    let receive: ((event: PreviewProjectionChangedEvent) => void) | null = null;
    const target = {
      projectId: "project-a",
      projectGeneration: "0",
      expectedTransitionEpoch: 0,
      chatId: "chat-focused",
      sessionId: "session-a",
      runtimeEpoch: "3",
      rootId: "root-a",
      scriptName: "start-a-very-long-preview-script-name-that-stays-accessible",
    } as const;
    previewBridge.subscribe.mockImplementation(async (handler) => {
      receive = handler;
      return vi.fn();
    });
    previewBridge.discover.mockResolvedValue({ targets: [target] });
    render(<AgentArtifactPane />);
    fireEvent.click(screen.getByRole("tab", { name: "Preview" }));
    await settleLazyModules();

    const targets = await screen.findByRole("group", {
      name: "Preview targets",
    });
    const row = targets.querySelector(".agent-preview-target");
    if (!(row instanceof HTMLElement))
      throw new Error("Preview target row was unavailable");
    expect(row.querySelector(".agent-preview-target__name")).toHaveAttribute(
      "title",
      target.scriptName,
    );
    expect(within(row).getByRole("button", { name: "Start" })).toHaveClass(
      "button",
      "button--compact",
      "button--primary",
    );
    expect(document.querySelector(".agent-preview-stack")).toHaveAttribute(
      "data-preview-listener-ready",
      "true",
    );
    expect(document.querySelector(".agent-preview-stack")).toHaveAttribute(
      "data-preview-phase",
      "none",
    );

    act(() =>
      receive?.({
        kind: "preview",
        preview: {
          ...previewOwner,
          previewRevision: "2",
          phase: "healthFailed",
          failure: "healthTimeout",
          target,
          origin: null,
        },
      } as unknown as PreviewProjectionChangedEvent),
    );
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(screen.getByRole("alert")).toHaveTextContent("Preview failed");
    expect(screen.getByRole("alert")).toHaveTextContent("Health timeout");
    expect(screen.queryByText("Preview is unavailable")).toBeNull();
    expect(document.querySelector(".agent-preview-stack")).toHaveAttribute(
      "data-preview-phase",
      "healthFailed",
    );
    expect(document.querySelector(".agent-preview-stack")).toHaveAttribute(
      "data-preview-target-identity",
      `${target.rootId}:${target.scriptName}`,
    );
  });

  it("stops the captured target exactly once when focused ownership disposes the controller", async () => {
    openPreviewSession();
    let receive: ((event: PreviewProjectionChangedEvent) => void) | null = null;
    previewBridge.subscribe.mockImplementation(async (handler) => {
      receive = handler;
      return vi.fn();
    });
    const target = {
      projectId: "project-a",
      projectGeneration: "0",
      expectedTransitionEpoch: 0,
      chatId: "chat-focused",
      sessionId: "session-a",
      runtimeEpoch: "3",
      rootId: "root-a",
      scriptName: "dev",
    } as const;
    render(<AgentArtifactPane />);
    fireEvent.click(screen.getByRole("tab", { name: "Preview" }));
    await settleLazyModules();
    await waitFor(() => expect(receive).not.toBeNull());
    act(() =>
      receive?.({
        kind: "preview",
        preview: {
          ...previewOwner,
          previewRevision: "2",
          phase: "active",
          failure: null,
          target,
          origin: "http://127.0.0.1:5173/native",
        },
      } as PreviewProjectionChangedEvent),
    );
    act(() => pinAgentChatOwner(null));
    await waitFor(() =>
      expect(previewBridge.stop).toHaveBeenCalledExactlyOnceWith(target),
    );
  });

  it("bridges active native Preview and hides only its matching-origin terminal frame", async () => {
    openPreviewSession();
    let receive: ((event: PreviewProjectionChangedEvent) => void) | null = null;
    previewBridge.subscribe.mockImplementation(async (handler) => {
      receive = handler;
      return vi.fn();
    });
    const target = {
      projectId: "project-a",
      projectGeneration: "0",
      expectedTransitionEpoch: 0,
      chatId: "chat-focused",
      sessionId: "session-a",
      runtimeEpoch: "3",
      rootId: "root-a",
      scriptName: "dev",
    } as const;
    previewBridge.discover.mockResolvedValue({ targets: [target] });
    const view = render(<AgentArtifactPane />);
    fireEvent.click(screen.getByRole("tab", { name: "Preview" }));
    await settleLazyModules();
    await waitFor(() => expect(receive).not.toBeNull());
    act(() =>
      receive?.({
        kind: "preview",
        preview: {
          ...previewOwner,
          previewRevision: "2",
          phase: "active",
          failure: null,
          target,
          origin: "http://127.0.0.1:5173/native",
        },
      } as PreviewProjectionChangedEvent),
    );
    await waitFor(() =>
      expect(screen.getByTitle("Sage browser")).toHaveAttribute(
        "src",
        "http://127.0.0.1:5173/native",
      ),
    );
    fireEvent.click(screen.getByRole("tab", { name: "File src/app.ts" }));
    fireEvent.click(screen.getByRole("tab", { name: "Preview" }));
    await settleLazyModules();
    await waitFor(() => expect(previewBridge.health).toHaveBeenCalledOnce());
    act(() =>
      receive?.({
        kind: "preview",
        preview: {
          ...previewOwner,
          previewRevision: "3",
          phase: "healthChecking",
          failure: null,
          target,
          origin: null,
        },
      } as PreviewProjectionChangedEvent),
    );
    act(() =>
      receive?.({
        kind: "preview",
        preview: {
          ...previewOwner,
          previewRevision: "4",
          phase: "active",
          failure: null,
          target,
          origin: "http://127.0.0.1:5173/native",
        },
      } as PreviewProjectionChangedEvent),
    );
    expect(previewBridge.health).toHaveBeenCalledOnce();

    act(() =>
      applyBrowseIntent({
        owner: previewOwner,
        url: "http://127.0.0.1:5174/manual",
      }),
    );
    act(() =>
      receive?.({
        kind: "preview",
        preview: {
          ...previewOwner,
          previewRevision: "5",
          phase: "active",
          failure: null,
          target,
          origin: "http://127.0.0.1:5173/native",
        },
      } as PreviewProjectionChangedEvent),
    );
    expect(screen.getByTitle("Sage browser")).toHaveAttribute(
      "src",
      "http://127.0.0.1:5174/manual",
    );
    act(() =>
      receive?.({
        kind: "preview",
        preview: {
          ...previewOwner,
          previewRevision: "6",
          phase: "healthChecking",
          failure: null,
          target,
          origin: null,
        },
      } as PreviewProjectionChangedEvent),
    );
    act(() =>
      receive?.({
        kind: "preview",
        preview: {
          ...previewOwner,
          previewRevision: "7",
          phase: "healthFailed",
          failure: "healthTimeout",
          target,
          origin: null,
        },
      } as PreviewProjectionChangedEvent),
    );
    expect(screen.getByTitle("Sage browser")).toHaveAttribute(
      "src",
      "http://127.0.0.1:5174/manual",
    );
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(screen.getByRole("alert")).toHaveTextContent("Preview failed");
    expect(screen.getByRole("alert")).toHaveTextContent("Health timeout");
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Preview did not become healthy before its deadline.",
    );
    previewBridge.start.mockRejectedValueOnce(
      new Error("retry request failed"),
    );
    fireEvent.click(screen.getByRole("button", { name: "Start" }));
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(
        "retry request failed",
      ),
    );
    expect(screen.getAllByRole("alert")).toHaveLength(1);

    act(() =>
      applyBrowseIntent({
        owner: previewOwner,
        url: "http://127.0.0.1:5173/other",
      }),
    );
    expect(screen.queryByTitle("Sage browser")).toBeNull();
    expect(
      screen
        .getAllByRole("alert")
        .some((alert) =>
          alert.textContent?.includes(
            "Preview did not become healthy before its deadline.",
          ),
        ),
    ).toBe(true);
    view.unmount();
    await waitFor(() =>
      expect(previewBridge.stop).toHaveBeenCalledExactlyOnceWith(target),
    );
  });

  it("suppresses a stopped native frame while preserving a different manual origin", async () => {
    openPreviewSession();
    let receive: ((event: PreviewProjectionChangedEvent) => void) | null = null;
    previewBridge.subscribe.mockImplementation(async (handler) => {
      receive = handler;
      return vi.fn();
    });
    const target = {
      projectId: "project-a",
      projectGeneration: "0",
      expectedTransitionEpoch: 0,
      chatId: "chat-focused",
      sessionId: "session-a",
      runtimeEpoch: "3",
      rootId: "root-a",
      scriptName: "dev",
    } as const;
    const emit = (
      revision: string,
      phase: PreviewProjectionChangedEvent["preview"]["phase"],
      origin: string | null,
    ) =>
      act(() =>
        receive?.({
          kind: "preview",
          preview: {
            ...previewOwner,
            previewRevision: revision,
            phase,
            failure: null,
            target,
            origin,
          },
        } as PreviewProjectionChangedEvent),
      );

    render(<AgentArtifactPane />);
    fireEvent.click(screen.getByRole("tab", { name: "Preview" }));
    await settleLazyModules();
    await waitFor(() => expect(receive).not.toBeNull());
    emit("2", "active", "http://127.0.0.1:5173/native");
    await waitFor(() =>
      expect(screen.getByTitle("Sage browser")).toHaveAttribute(
        "src",
        "http://127.0.0.1:5173/native",
      ),
    );
    emit("3", "stopping", null);
    expect(screen.queryByTitle("Sage browser")).toBeNull();
    expect(screen.getByText("Stopping process")).toBeInTheDocument();
    emit("4", "stopped", null);
    expect(screen.queryByTitle("Sage browser")).toBeNull();
    expect(screen.getByText("Preview stopped")).toBeInTheDocument();

    emit("5", "starting", null);
    emit("6", "waitingForOrigin", null);
    emit("7", "healthChecking", null);
    emit("8", "active", "http://127.0.0.1:5173/native");
    await waitFor(() =>
      expect(screen.getByTitle("Sage browser")).toHaveAttribute(
        "src",
        "http://127.0.0.1:5173/native",
      ),
    );
    act(() =>
      applyBrowseIntent({
        owner: previewOwner,
        url: "http://127.0.0.1:5174/manual",
      }),
    );
    emit("9", "stopping", null);
    expect(screen.getByTitle("Sage browser")).toHaveAttribute(
      "src",
      "http://127.0.0.1:5174/manual",
    );
    emit("10", "stopped", null);
    expect(screen.getByTitle("Sage browser")).toHaveAttribute(
      "src",
      "http://127.0.0.1:5174/manual",
    );
  });

  it("uses only emitted phases for first-native notices and leaves idle native states as the browser shell", async () => {
    openPreviewSession();
    let receive: ((event: PreviewProjectionChangedEvent) => void) | null = null;
    previewBridge.subscribe.mockImplementation(async (handler) => {
      receive = handler;
      return vi.fn();
    });
    render(<AgentArtifactPane />);
    fireEvent.click(screen.getByRole("tab", { name: "Preview" }));
    await settleLazyModules();
    await waitFor(() => expect(receive).not.toBeNull());
    const target = {
      projectId: "project-a",
      projectGeneration: "0",
      expectedTransitionEpoch: 0,
      chatId: "chat-focused",
      sessionId: "session-a",
      runtimeEpoch: "3",
      rootId: "root-a",
      scriptName: "dev",
    } as const;
    act(() =>
      receive?.({
        kind: "preview",
        preview: {
          ...previewOwner,
          previewRevision: "2",
          phase: "starting",
          failure: null,
          target,
          origin: null,
        },
      } as PreviewProjectionChangedEvent),
    );
    expect(screen.getByText("Starting process")).toBeInTheDocument();
    act(() =>
      receive?.({
        kind: "preview",
        preview: {
          ...previewOwner,
          previewRevision: "3",
          phase: "starting",
          failure: "conflict",
          target,
          origin: null,
        },
      } as PreviewProjectionChangedEvent),
    );
    const conflict = screen
      .getByText("Ownership conflict")
      .closest('[role="status"]');
    expect(conflict).toHaveTextContent("Starting process");
    expect(conflict).toHaveTextContent("Ownership conflict");
    act(() =>
      receive?.({
        kind: "preview",
        preview: {
          ...previewOwner,
          previewRevision: "4",
          phase: "healthFailed",
          failure: "healthTimeout",
          target,
          origin: null,
        },
      } as PreviewProjectionChangedEvent),
    );
    expect(screen.getByText("Preview failed")).toBeInTheDocument();
    act(() =>
      receive?.({
        kind: "preview",
        preview: {
          ...previewOwner,
          previewRevision: "5",
          phase: "starting",
          failure: null,
          target,
          origin: null,
        },
      } as PreviewProjectionChangedEvent),
    );
    act(() =>
      receive?.({
        kind: "preview",
        preview: {
          ...previewOwner,
          previewRevision: "6",
          phase: "exited",
          failure: "exitedEarly",
          target,
          origin: null,
        },
      } as PreviewProjectionChangedEvent),
    );
    expect(screen.getByText("Preview exited")).toBeInTheDocument();
    expect(screen.getByText("Early exit")).toBeInTheDocument();
    act(() =>
      receive?.({
        kind: "preview",
        preview: {
          ...previewOwner,
          previewRevision: "7",
          phase: "starting",
          failure: null,
          target,
          origin: null,
        },
      } as PreviewProjectionChangedEvent),
    );
    act(() =>
      receive?.({
        kind: "preview",
        preview: {
          ...previewOwner,
          previewRevision: "8",
          phase: "stopping",
          failure: null,
          target,
          origin: null,
        },
      } as PreviewProjectionChangedEvent),
    );
    expect(screen.getByText("Stopping process")).toBeInTheDocument();
    expect(
      screen.getByText("Stopping the native Preview process."),
    ).toBeInTheDocument();
    act(() =>
      receive?.({
        kind: "preview",
        preview: {
          ...previewOwner,
          previewRevision: "9",
          phase: "stopped",
          failure: null,
          target,
          origin: null,
        },
      } as PreviewProjectionChangedEvent),
    );
    expect(screen.getByText("Preview stopped")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Preview has stopped. Start it again when you are ready.",
      ),
    ).toBeInTheDocument();
  });

  it("keeps a pinned File when Preview arrives and resumes Follow latest", () => {
    useAgentArtifactFollowStore
      .getState()
      .pin(focusedOwner, { kind: "tab", tabId: "file:app" });
    render(<AgentArtifactPane />);

    expect(screen.getByRole("tab", { name: /app\.ts/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    act(() => {
      applyBrowseIntent({
        owner: previewOwner,
        url: "http://127.0.0.1:5173/preview",
      });
    });

    expect(screen.getByRole("tab", { name: "Preview" })).toHaveAttribute(
      "aria-selected",
      "false",
    );
    const follow = screen.getByRole("button", {
      name: "Pinned resource, new Preview available",
    });
    expect(follow).toHaveAttribute("aria-pressed", "false");
    expect(follow).toHaveTextContent("Pinned");
    expect(follow).toHaveTextContent("New");
    expect(
      screen.getByRole("tab", { name: "Preview" }),
    ).toHaveAccessibleDescription("New resource");
    fireEvent.click(follow);
    expect(follow).toHaveAttribute("aria-pressed", "true");
    expect(follow).toHaveAccessibleName("Following latest resource");
    expect(screen.getByRole("tab", { name: "Preview" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    act(() => clearBrowseSession(previewOwner));
    expect(screen.queryByRole("tab", { name: "Preview" })).toBeNull();
  });

  it("keeps a spaced tab id connected to its pending description", () => {
    useWorkbenchStore.setState((current) => ({
      tabs: [
        ...current.tabs,
        {
          id: "diff:root-a:src/new file.ts",
          kind: "diff",
          title: "new file.ts",
          position: current.tabs.length,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "src/new file.ts" },
        },
      ],
    }));
    admit("diff:root-a:src/new file.ts");
    useAgentArtifactFollowStore
      .getState()
      .pin(focusedOwner, { kind: "tab", tabId: "file:app" });
    useAgentArtifactFollowStore.getState().observe(focusedOwner, {
      kind: "tab",
      tabId: "diff:root-a:src/new file.ts",
    });
    render(<AgentArtifactPane />);

    const pending = screen.getByRole("tab", { name: "Diff src/new file.ts" });
    expect(pending).toHaveAccessibleDescription("New resource");
    expect(pending.getAttribute("aria-describedby")).toMatch(
      /^agent-artifact-pending-[0-9]+$/,
    );
  });

  it("pins an explicitly selected resource tab for the focused Chat", () => {
    admit("diff:other");
    render(<AgentArtifactPane />);
    fireEvent.click(screen.getByRole("tab", { name: /other\.ts$/ }));

    expect(
      agentArtifactFollowRecordFor(
        useAgentArtifactFollowStore.getState().records,
        focusedOwner,
      ),
    ).toMatchObject({
      mode: "pinned",
      selected: { kind: "tab", tabId: "diff:other" },
    });
  });

  it("shows an Agent-admitted unmatched tab without edit-summary counters", () => {
    admit("diff:other");
    render(<AgentArtifactPane />);

    const unmatched = screen.getByRole("tab", { name: /other\.ts$/ });
    expect(within(unmatched).queryByText("+3")).toBeNull();
    expect(within(unmatched).queryByText("-1")).toBeNull();
  });

  it("hides an admission during Project transition and after session replacement", () => {
    admit("diff:other");
    render(<AgentArtifactPane />);
    expect(screen.getByRole("tab", { name: /other\.ts$/ })).toBeVisible();

    act(() => useWorkbenchStore.setState({ projectTransitioning: true }));
    expect(screen.queryByRole("tab", { name: /other\.ts$/ })).toBeNull();

    act(() =>
      useWorkbenchStore.setState({
        projectTransitioning: false,
        projectGeneration: 1,
      }),
    );
    expect(screen.queryByRole("tab", { name: /other\.ts$/ })).toBeNull();

    act(() => {
      useWorkbenchStore.setState({ projectGeneration: 0 });
      const replacementOwner = {
        ...focusedOwner,
        sessionId: "session-new",
      } as AgentArtifactOwner;
      acpProjectionStoreFor(focusedOwner.chatId).setState({
        page: {
          ...pageWithEdit(),
          currentSessionId: replacementOwner.sessionId,
        },
      });
    });

    expect(screen.queryByRole("tab", { name: /other\.ts$/ })).toBeNull();
  });

  it("uses language-aware file icons on file tabs and keeps the diff glyph", () => {
    useWorkbenchStore.setState({
      tabs: [
        {
          id: "chat-focused",
          kind: "chat",
          title: "Focused",
          position: 0,
          pinned: false,
          dirty: false,
        },
        {
          id: "file:app",
          kind: "file",
          title: "app.ts",
          position: 1,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "src/app.ts" },
        },
        {
          id: "file:readme",
          kind: "file",
          title: "README.md",
          position: 2,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "README.md" },
        },
        {
          id: "diff:other",
          kind: "diff",
          title: "other.ts",
          position: 3,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "src/other.ts" },
        },
      ],
      activeTabId: "chat-focused",
    });
    admit("file:readme");
    admit("diff:other");
    render(<AgentArtifactPane />);

    expect(
      screen
        .getByRole("tab", { name: /app\.ts/ })
        .querySelector(".file-kind-icon--typescript"),
    ).not.toBeNull();
    expect(screen.getByRole("tab", { name: /app\.ts/ })).toHaveAttribute(
      "data-resource-root-id",
      "root-a",
    );
    expect(screen.getByRole("tab", { name: /app\.ts/ })).toHaveAttribute(
      "data-resource-relative-path",
      "src/app.ts",
    );
    expect(
      screen
        .getByRole("tab", { name: /README\.md/ })
        .querySelector(".file-kind-icon--markdown"),
    ).not.toBeNull();
    expect(
      screen
        .getByRole("tab", { name: /other\.ts$/ })
        .querySelector(".file-kind-icon"),
    ).toBeNull();
  });

  it("shows +3 −1 on a matched diff tab", () => {
    useWorkbenchStore.setState({
      tabs: [
        {
          id: "chat-focused",
          kind: "chat",
          title: "Focused",
          position: 0,
          pinned: false,
          dirty: false,
        },
        {
          id: "diff:app",
          kind: "diff",
          title: "app.ts",
          position: 1,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "src/app.ts" },
        },
      ],
      activeTabId: "chat-focused",
    });
    render(<AgentArtifactPane />);

    const matched = screen.getByRole("tab", { name: /app\.ts/ });
    expect(matched).toHaveAttribute("aria-label", "Diff src/app.ts");
    expect(within(matched).getByText("+3")).toBeInTheDocument();
    expect(within(matched).getByText("-1")).toBeInTheDocument();
  });

  it("shows a dirty dot only on a dirty file tab", () => {
    useWorkbenchStore.setState({
      tabs: [
        {
          id: "chat-focused",
          kind: "chat",
          title: "Focused",
          position: 0,
          pinned: false,
          dirty: false,
        },
        {
          id: "file:app",
          kind: "file",
          title: "app.ts",
          position: 1,
          pinned: false,
          dirty: true,
          resource: { rootId: "root-a", relativePath: "src/app.ts" },
        },
        {
          id: "file:readme",
          kind: "file",
          title: "README.md",
          position: 2,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "README.md" },
        },
        {
          id: "diff:other",
          kind: "diff",
          title: "other.ts",
          position: 3,
          pinned: false,
          dirty: false,
          resource: { rootId: "root-a", relativePath: "src/other.ts" },
        },
      ],
      activeTabId: "file:app",
    });
    admit("file:readme");
    admit("diff:other");
    render(<AgentArtifactPane />);

    expect(
      within(screen.getByRole("tab", { name: /app\.ts/ })).getByTitle(
        "Unsaved changes",
      ),
    ).toBeInTheDocument();
    expect(
      within(screen.getByRole("tab", { name: /README\.md/ })).queryByTitle(
        "Unsaved changes",
      ),
    ).toBeNull();
    expect(
      within(screen.getByRole("tab", { name: /other\.ts$/ })).queryByTitle(
        "Unsaved changes",
      ),
    ).toBeNull();
    expect(screen.queryByRole("tab", { name: "Preview" })).toBeNull();
  });

  it("closes an Agent artifact without adding a close control to Preview", () => {
    render(<AgentArtifactPane />);

    fireEvent.click(screen.getByRole("button", { name: "Close app.ts" }));
    expect(screen.queryByRole("tab", { name: /app\.ts/ })).toBeNull();
    expect(screen.queryByRole("button", { name: "Close Preview" })).toBeNull();
  });

  it("disables browser creation without the desktop runtime after collapse and expand", async () => {
    const openBrowser = vi.fn(async () => undefined);
    useWorkbenchStore.setState({ openBrowser });
    const { rerender } = render(<AgentArtifactPane />);
    expect(
      screen.getByRole("button", { name: "New browser tab" }),
    ).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "New browser tab" }));
    expect(openBrowser).not.toHaveBeenCalled();

    rerender(<AgentArtifactPane expanded={false} />);
    await settleLazyModules();
    rerender(<AgentArtifactPane expanded />);
    await settleLazyModules();
    expect(
      screen.getByRole("button", { name: "New browser tab" }),
    ).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "New browser tab" }));
    expect(openBrowser).not.toHaveBeenCalled();
  });

  it("keeps a pinned Diff and pending Preview when an agent opens a browser tab", async () => {
    vi.stubGlobal("__TAURI_INTERNALS__", {});
    browserEvents.listen.mockReset().mockResolvedValue(() => undefined);
    createBrowserSlice(useWorkbenchStore.setState, useWorkbenchStore.getState, {
      subscribe: () => () => undefined,
    } as never);
    let onAgentOpened: (event: {
      payload: { projectId: string; tabId: string; url: string };
    }) => void;
    await waitFor(() => {
      onAgentOpened = browserEvents.listen.mock.calls.find(
        ([name]) => name === "browser:agent-opened",
      )?.[1];
      if (!onAgentOpened) throw new Error("Agent browser listener not ready");
    });
    admit("diff:other");
    render(<AgentArtifactPane />);
    const diff = screen.getByRole("tab", { name: "Diff src/other.ts" });
    fireEvent.click(diff);
    act(openPreviewSession);

    act(() => {
      onAgentOpened({
        payload: {
          projectId: "project-a",
          tabId: "browser:agent",
          url: "https://example.com/agent",
        },
      });
    });

    expect(diff).toHaveAttribute("aria-selected", "true");
    expect(
      screen.getByRole("tab", { name: "Browser example.com" }),
    ).toHaveAttribute("aria-selected", "false");
    expect(
      screen.getByRole("tab", { name: "Preview" }),
    ).toHaveAccessibleDescription("New resource");
    expect(
      screen.getByRole("button", {
        name: "Pinned resource, new Preview available",
      }),
    ).toBeVisible();
  });

  it.each(["follow", "pinned"])(
    "selects the tab that user + opens over a %s agent Diff",
    async (mode) => {
      vi.stubGlobal("__TAURI_INTERNALS__", {});
      nativeBrowser.open.mockResolvedValueOnce("browser:opened");
      admit("diff:other");
      const follow = useAgentArtifactFollowStore.getState();
      follow[mode === "pinned" ? "pin" : "observe"](focusedOwner, {
        kind: "tab",
        tabId: "diff:other",
      });
      useWorkbenchStore.setState({ activeTabId: "diff:other" });
      render(<AgentArtifactPane />);
      expect(
        screen.getByRole("tab", { name: "Diff src/other.ts" }),
      ).toHaveAttribute("aria-selected", "true");

      fireEvent.click(screen.getByRole("button", { name: "New browser tab" }));

      const opened = await screen.findByRole("tab", {
        name: "Browser New tab",
      });
      await waitFor(() =>
        expect(opened).toHaveAttribute("aria-selected", "true"),
      );
      expect(
        screen.getByRole("tab", { name: "Diff src/other.ts" }),
      ).toHaveAttribute("aria-selected", "false");
      expect(
        screen.getByRole("button", { name: "Pinned resource" }),
      ).toHaveAttribute("aria-pressed", "false");
    },
  );

  it.each(["unattached", "user takeover", "agent handback"])(
    "inherits popup selection intent from the opener: %s",
    async (control) => {
      vi.stubGlobal("__TAURI_INTERNALS__", {});
      nativeBrowser.open.mockResolvedValueOnce("browser:opener");
      nativeBrowser.control.mockResolvedValue(
        control === "unattached"
          ? null
          : {
              projectId: "project-a",
              chatId: "chat-focused",
              userControl: true,
            },
      );
      nativeBrowser.popupPending.mockResolvedValue([]);
      nativeBrowser.popupAck.mockResolvedValue(undefined);
      browserEvents.listen.mockReset().mockResolvedValue(() => undefined);
      createBrowserSlice(
        useWorkbenchStore.setState,
        useWorkbenchStore.getState,
        {
          subscribe: () => () => undefined,
        } as never,
      );
      let onPopup: (event: {
        payload: { id: number; openerTabId: string; url: string };
      }) => void;
      await waitFor(() => {
        onPopup = browserEvents.listen.mock.calls.find(
          ([name]) => name === "browser:popup-request",
        )?.[1];
        if (!onPopup) throw new Error("Popup listener not ready");
      });
      admit("diff:other");
      render(<AgentArtifactPane />);
      fireEvent.click(screen.getByRole("button", { name: "New browser tab" }));
      await waitFor(() =>
        expect(
          screen.getByRole("tab", { name: "Browser New tab" }),
        ).toHaveAttribute("aria-selected", "true"),
      );
      nativeBrowser.popupOpen.mockResolvedValueOnce({
        tabId: "browser:popup",
        openerTabId: "browser:opener",
        url: "https://signin.example.com/",
      });
      act(() =>
        onPopup({
          payload: {
            id: 1,
            openerTabId: "browser:opener",
            url: "https://signin.example.com/",
          },
        }),
      );
      const popup = await screen.findByRole("tab", {
        name: "Browser signin.example.com",
      });
      await waitFor(() =>
        expect(popup).toHaveAttribute("aria-selected", "true"),
      );

      if (control !== "agent handback") return;
      const diff = screen.getByRole("tab", { name: "Diff src/other.ts" });
      fireEvent.click(diff);
      nativeBrowser.control.mockResolvedValue({
        projectId: "project-a",
        chatId: "chat-focused",
        userControl: false,
      });
      nativeBrowser.popupOpen.mockResolvedValueOnce({
        tabId: "browser:agent-popup",
        openerTabId: "browser:opener",
        url: "https://agent.example.com/",
      });
      act(() =>
        onPopup({
          payload: {
            id: 2,
            openerTabId: "browser:opener",
            url: "https://agent.example.com/",
          },
        }),
      );
      const agentPopup = await screen.findByRole("tab", {
        name: "Browser agent.example.com",
      });
      expect(agentPopup).toHaveAttribute("aria-selected", "false");
      expect(diff).toHaveAttribute("aria-selected", "true");
    },
  );

  it("shows browser tabs beside artifacts but keeps them out of Editor", () => {
    vi.stubGlobal("__TAURI_INTERNALS__", {});
    const openBrowser = vi.fn(async () => undefined);
    const onOpenInEditor = vi.fn();
    useWorkbenchStore.setState((state) => ({
      tabs: [
        ...state.tabs,
        {
          id: "browser:a",
          kind: "browser",
          title: "New tab",
          favicon: "data:image/png;base64,iVBORw0KGgo=",
          position: 8,
          pinned: false,
          dirty: false,
        },
      ],
      openBrowser,
    }));
    render(<AgentArtifactPane onOpenInEditor={onOpenInEditor} />);
    expect(
      screen.getByRole("tab", { name: "Browser New tab" }),
    ).toBeInTheDocument();
    expect(
      screen
        .getByRole("tab", { name: "Browser New tab" })
        .querySelector("img.tab__favicon"),
    ).toHaveAttribute("src", "data:image/png;base64,iVBORw0KGgo=");
    fireEvent.error(
      screen
        .getByRole("tab", { name: "Browser New tab" })
        .querySelector("img.tab__favicon")!,
    );
    expect(
      screen
        .getByRole("tab", { name: "Browser New tab" })
        .querySelector("svg.lucide"),
    ).toBeInTheDocument();
    act(() =>
      useWorkbenchStore.setState((state) => ({
        tabs: state.tabs.map((item) =>
          item.id === "browser:a"
            ? { ...item, favicon: "data:image/png;base64,iVBORw0KGgoAAA=" }
            : item,
        ),
      })),
    );
    expect(
      screen
        .getByRole("tab", { name: "Browser New tab" })
        .querySelector("img.tab__favicon"),
    ).toHaveAttribute("src", "data:image/png;base64,iVBORw0KGgoAAA=");
    expect(
      within(screen.getByRole("tab", { name: "Browser New tab" })).queryByText(
        "Browser",
      ),
    ).toBeNull();
    fireEvent.click(screen.getByRole("tab", { name: "Browser New tab" }));
    fireEvent.click(screen.getByRole("button", { name: "New browser tab" }));
    expect(openBrowser).toHaveBeenCalledTimes(1);
    expect(
      screen.getByRole("button", { name: "Expand resource pane" }),
    ).toBeEnabled();
    expect(onOpenInEditor).not.toHaveBeenCalled();
  });

  it("keeps the newest browser selected after returning to its Chat", () => {
    useWorkbenchStore.setState((state) => ({
      tabs: [
        ...state.tabs,
        {
          id: "browser:example",
          kind: "browser",
          title: "Example Domain",
          position: 3,
          pinned: false,
          dirty: false,
        },
        {
          id: "browser:wiki",
          kind: "browser",
          title: "Web browser - Wikipedia",
          position: 4,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-focused",
    }));

    render(<AgentArtifactPane />);

    expect(
      screen.getByRole("tab", { name: "Browser Web browser - Wikipedia" }),
    ).toHaveAttribute("aria-selected", "true");
    expect(
      screen.getByRole("tab", { name: "Browser Example Domain" }),
    ).toHaveAttribute("aria-selected", "false");
    expect(
      screen.queryByRole("heading", { name: "Waiting for this Chat" }),
    ).toBeNull();
  });

  it("keeps a project browser selected when switching between Chats", () => {
    pinAcpProjectionIds(["chat-focused", "chat-second"]);
    const secondStore = acpProjectionStoreFor("chat-second");
    const secondScope = {
      projectId: "project-a" as SageProjectId,
      projectGeneration: "0" as ProjectGeneration,
      chatId: "chat-second" as SageChatId,
    };
    secondStore.getState().activateScope(secondScope);
    expect(
      secondStore.getState().applyAuthoritativePage(secondScope, {
        projectId: "project-a",
        chatId: "chat-second",
        title: "Second",
        projectionRevision: "1",
        currentSessionId: "session-b",
        sessions: [{ id: "session-b", ordinal: 1 }],
        turns: [],
        nextCursor: null,
      } as unknown as AcpChatPage),
    ).toBe("accepted");
    useWorkbenchStore.setState((state) => ({
      tabs: [
        ...state.tabs,
        {
          id: "chat-second",
          kind: "chat",
          title: "Second",
          position: 3,
          pinned: false,
          dirty: false,
        },
        {
          id: "browser:shared",
          kind: "browser",
          title: "Example Domain",
          position: 4,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-focused",
    }));

    render(<AgentArtifactPane />);
    const browserTab = screen.getByRole("tab", {
      name: "Browser Example Domain",
    });
    expect(browserTab).toHaveAttribute("aria-selected", "true");

    act(() => {
      pinAgentChatOwner("chat-second");
      useWorkbenchStore.setState({ activeTabId: "chat-second" });
    });
    expect(browserTab).toHaveAttribute("aria-selected", "true");
    expect(
      screen.queryByRole("heading", { name: "Waiting for this Chat" }),
    ).toBeNull();
  });

  it("exposes full paths in accessible tab names with basename-visible middle ellipsis", () => {
    admit("diff:other");
    render(<AgentArtifactPane />);

    const fileTab = screen.getByRole("tab", { name: "File src/app.ts" });
    expect(fileTab).toHaveAttribute("title", "File src/app.ts");
    const filePath = fileTab.querySelector(".tab__path");
    expect(filePath).not.toBeNull();
    expect(filePath?.querySelector(".tab__dir")?.textContent).toBe("src/");
    expect(filePath?.querySelector(".tab__basename")?.textContent).toBe(
      "app.ts",
    );

    const diffTab = screen.getByRole("tab", { name: "Diff src/other.ts" });
    expect(diffTab).toHaveAttribute("title", "Diff src/other.ts");
    expect(diffTab.querySelector(".tab__basename")?.textContent).toBe(
      "other.ts",
    );
  });

  it("does not allocate a collapsed resource edge without a Chat header", () => {
    const { container } = render(<AgentArtifactPane expanded={false} />);
    expect(container.querySelector(".agent-artifact-pane")).toBeNull();
    expect(screen.queryByRole("button", { name: /Show/ })).toBeNull();
  });

  it("names the header trigger for all resources and controls the opened pane", () => {
    const header = render(<div data-testid="resource-header" />);
    const headerNode = header.getByTestId("resource-header");
    const view = render(
      <AgentArtifactPane
        expanded={false}
        focusOnExpand
        headerNode={headerNode}
      />,
    );
    const trigger = screen.getByRole("button", {
      name: /Show files and preview/,
    });
    expect(trigger).toHaveAttribute("aria-haspopup", "dialog");
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    const paneId = trigger.getAttribute("aria-controls");
    view.rerender(
      <AgentArtifactPane expanded focusOnExpand headerNode={headerNode} />,
    );
    expect(
      screen.getByRole("dialog", { name: "Agent artifacts" }),
    ).toHaveAttribute("id", paneId);
  });

  it("keeps iframe exit focus within the resource overlay", async () => {
    openPreviewSession();
    render(<AgentArtifactPane focusOnExpand />);
    fireEvent.click(screen.getByRole("tab", { name: "Preview" }));
    await settleLazyModules();
    const pane = screen.getByRole("dialog", { name: "Agent artifacts" });
    screen.getByTitle("Sage browser").focus();
    const guards = pane.querySelectorAll<HTMLElement>(
      "[data-artifact-focus-guard]",
    );
    expect(guards).toHaveLength(2);
    guards[1].focus();
    expect(pane.contains(document.activeElement)).toBe(true);
    guards[0].focus();
    expect(pane.contains(document.activeElement)).toBe(true);
  });

  it("names the collapsed toggle with a truthful resource count badge", () => {
    const onExpandedChange = vi.fn();
    render(
      <AgentArtifactPane
        expanded={false}
        headerNode={mountArtifactHeader()}
        onExpandedChange={onExpandedChange}
      />,
    );

    const toggle = screen.getByRole("button", {
      name: "Show files and preview · Ready · 1 resource",
    });
    expect(toggle).toHaveAttribute(
      "title",
      "Show files and preview · Ready · 1 resource",
    );
    expect(within(toggle).getByText("1")).toBeInTheDocument();
  });

  it("keeps the collapsed pane compact while announcing its ready state", () => {
    render(
      <AgentArtifactPane expanded={false} headerNode={mountArtifactHeader()} />,
    );
    const toggle = screen.getByRole("button", {
      name: /Show files and preview · Ready · 1 resource/,
    });
    expect(toggle).toHaveAttribute(
      "title",
      "Show files and preview · Ready · 1 resource",
    );
    expect(toggle.querySelector(".agent-artifact-pane__edge-copy")).toBeNull();
    expect(toggle).not.toHaveTextContent("Ready");
    expect(within(toggle).getByText("1")).toBeInTheDocument();
  });

  it("shows Preview for a browse session and removes it when the browser closes", async () => {
    openPreviewSession();
    render(<AgentArtifactPane />);

    fireEvent.click(screen.getByRole("tab", { name: "Preview" }));
    await settleLazyModules();
    expect(screen.getByTitle("Sage browser")).toHaveAttribute(
      "src",
      "http://127.0.0.1:5173/native",
    );

    fireEvent.click(screen.getByRole("button", { name: "Close browser" }));
    expect(screen.queryByTitle("Sage browser")).toBeNull();
    expect(screen.queryByRole("tab", { name: "Preview" })).toBeNull();
  });

  it("keeps Preview sessions isolated per Chat in the actual pane", async () => {
    const secondOwner = {
      projectId: "project-a",
      projectGeneration: "0",
      transitionEpoch: 0,
      chatId: "chat-second",
      sessionId: "session-b",
      runtimeEpoch: "3",
    } as HarnessBrowseOwner;
    pinAcpProjectionIds(["chat-focused", "chat-second"]);
    const secondStore = acpProjectionStoreFor("chat-second");
    const secondScope = {
      projectId: "project-a" as SageProjectId,
      projectGeneration: "0" as ProjectGeneration,
      chatId: "chat-second" as SageChatId,
    };
    secondStore.getState().activateScope(secondScope);
    expect(
      secondStore.getState().applyAuthoritativePage(secondScope, {
        projectId: "project-a",
        chatId: "chat-second",
        title: "Second",
        projectionRevision: "1",
        currentSessionId: "session-b",
        sessions: [{ id: "session-b", ordinal: 1 }],
        turns: [],
        nextCursor: null,
      } as unknown as AcpChatPage),
    ).toBe("accepted");
    useWorkbenchStore.setState((state) => ({
      tabs: [
        ...state.tabs,
        {
          id: "chat-second",
          kind: "chat",
          title: "Second",
          position: 3,
          pinned: false,
          dirty: false,
        },
      ],
    }));
    applyBrowseIntent({
      owner: previewOwner,
      url: "http://127.0.0.1:5173/focused",
    });
    setHarnessBrowseOwner(secondOwner, ["chat-focused", "chat-second"]);
    applyBrowseIntent({
      owner: secondOwner,
      url: "http://127.0.0.1:5174/second",
    });
    render(<AgentArtifactPane />);
    // The pane shows a Preview only once its Chat owns the harness browse.
    expect(screen.queryByRole("tab", { name: "Preview" })).toBeNull();
    act(() =>
      setHarnessBrowseOwner(previewOwner, ["chat-focused", "chat-second"]),
    );
    fireEvent.click(screen.getByRole("tab", { name: "Preview" }));
    await settleLazyModules();
    expect(screen.getByTitle("Sage browser")).toHaveAttribute(
      "src",
      "http://127.0.0.1:5173/focused",
    );

    act(() => {
      pinAgentChatOwner(secondOwner.chatId);
      setHarnessBrowseOwner(secondOwner, ["chat-focused", "chat-second"]);
    });
    expect(screen.getByTitle("Sage browser")).toHaveAttribute(
      "src",
      "http://127.0.0.1:5174/second",
    );
    expect(screen.queryByTitle("Sage browser")?.getAttribute("src")).not.toBe(
      "http://127.0.0.1:5173/focused",
    );

    act(() => {
      pinAgentChatOwner(focusedOwner.chatId);
      setHarnessBrowseOwner(previewOwner, ["chat-focused", "chat-second"]);
    });
    expect(screen.getByTitle("Sage browser")).toHaveAttribute(
      "src",
      "http://127.0.0.1:5173/focused",
    );
  });

  it("dismisses overlay resources with Escape from portalled Preview chrome", async () => {
    const onExpandedChange = vi.fn();
    openPreviewSession();
    render(
      <AgentArtifactPane focusOnExpand onExpandedChange={onExpandedChange} />,
    );
    fireEvent.click(screen.getByRole("tab", { name: "Preview" }));
    await settleLazyModules();
    const reload = screen.getByRole("button", { name: "Reload preview" });
    reload.focus();
    fireEvent.keyDown(reload, { key: "Escape" });
    expect(onExpandedChange).toHaveBeenCalledWith(false);
  });

  it("keeps Preview chrome order and returns focus when collapsing", async () => {
    const onExpandedChange = vi.fn();
    applyBrowseIntent({
      owner: previewOwner,
      url: "http://127.0.0.1:5173/app",
    });
    render(<AgentArtifactPane expanded onExpandedChange={onExpandedChange} />);
    fireEvent.click(screen.getByRole("tab", { name: "Preview" }));
    await settleLazyModules();

    const chrome = screen.getByRole("region", { name: "Sage browser" });
    expect(
      Array.from(chrome.querySelectorAll("button, input")).map((element) =>
        element.getAttribute("aria-label"),
      ),
    ).toEqual([
      "Back",
      "Forward",
      "Reload preview",
      "Close browser",
      "Preview controls",
      "Preview address",
    ]);

    fireEvent.keyDown(screen.getByLabelText("Agent artifacts"), {
      key: "Escape",
    });
    expect(onExpandedChange).toHaveBeenLastCalledWith(false);
  });
});

describe("Agent artifact pane Changes ownership", () => {
  const lateScope = {
    projectId: "project-a" as SageProjectId,
    projectGeneration: "0" as ProjectGeneration,
    chatId: "chat-late" as SageChatId,
  };
  const pageFor = (sessionId: string | null) => {
    const page = JSON.parse(
      JSON.stringify(pageWithEdit()).replaceAll("chat-focused", "chat-late"),
    ) as AcpChatPage & { currentSessionId: string | null };
    page.currentSessionId = sessionId as AcpChatPage["currentSessionId"];
    // Starting a session is a newer projection revision.
    page.projectionRevision = (
      sessionId ? "9" : "8"
    ) as AcpChatPage["projectionRevision"];
    return page;
  };
  function focusLateChat() {
    pinAcpProjectionIds(["chat-focused", "chat-late"]);
    acpProjectionStoreFor("chat-late").getState().activateScope(lateScope);
    pinAgentChatOwner("chat-late");
    useWorkbenchStore.setState({
      tabs: [
        {
          id: "chat-late",
          kind: "chat",
          title: "Late",
          position: 0,
          pinned: false,
          dirty: false,
        },
      ],
      activeTabId: "chat-late",
    });
  }
  afterEach(() => {
    useAgentChangesState.getState().close();
    acpProjectionStoreFor("chat-late").getState().clear();
  });

  it("keeps a Chat with an unloaded projection waiting, then opens its session's Changes", async () => {
    focusLateChat();
    const { rerender } = render(<AgentArtifactPane expanded={false} />);
    rerender(<AgentArtifactPane expanded />);
    // The session is unknown until the projection loads: no invented review.
    expect(
      screen.getByRole("heading", { name: "Waiting for this Chat" }),
    ).toBeVisible();
    expect(useAgentChangesState.getState().owner).toBeNull();
    act(() => {
      acpProjectionStoreFor("chat-late")
        .getState()
        .applyAuthoritativePage(lateScope, pageFor("session-a"));
    });
    expect(useAgentChangesState.getState().owner).toEqual({
      projectId: "project-a",
      chatId: "chat-late",
      sessionId: "session-a",
    });
    expect(
      await screen.findByRole("region", { name: "Chat changes" }),
    ).toBeVisible();
  });

  it("adopts a started session into the sessionless tab and needs a loaded projection to reuse it", async () => {
    focusLateChat();
    const store = acpProjectionStoreFor("chat-late");
    act(() => {
      store.getState().applyAuthoritativePage(lateScope, pageFor(null));
    });
    const { rerender } = render(<AgentArtifactPane expanded={false} />);
    rerender(<AgentArtifactPane expanded />);
    expect(useAgentChangesState.getState().owner?.sessionId).toBeNull();
    // A session starts: the same tab now belongs to it, still selected, so
    // an exact-session open of one of its resources can reveal over it.
    act(() => {
      store.getState().applyAuthoritativePage(lateScope, pageFor("session-a"));
    });
    expect(useAgentChangesState.getState()).toMatchObject({
      owner: { chatId: "chat-late", sessionId: "session-a" },
      selected: true,
    });
    // Leaving releases the projection; the retained tab must not show a
    // review again until the Chat's projection reloads.
    act(() => store.getState().clear());
    expect(screen.queryByRole("region", { name: "Chat changes" })).toBeNull();
    expect(
      screen.getByRole("heading", { name: "Waiting for this Chat" }),
    ).toBeVisible();
  });

  it("expands for every explicit Changes request on a sessionless Chat", () => {
    focusLateChat();
    act(() => {
      acpProjectionStoreFor("chat-late")
        .getState()
        .applyAuthoritativePage(lateScope, pageFor(null));
    });
    const onExpandedChange = vi.fn();
    const sessionless = () => ({
      projectId: "project-a",
      chatId: "chat-late",
      sessionId: null,
    });
    const { rerender } = render(
      <AgentArtifactPane
        expanded={false}
        onExpandedChange={onExpandedChange}
      />,
    );
    act(() => useAgentChangesState.getState().show(sessionless(), 0));
    expect(onExpandedChange).toHaveBeenLastCalledWith(true);
    rerender(
      <AgentArtifactPane expanded onExpandedChange={onExpandedChange} />,
    );
    rerender(
      <AgentArtifactPane
        expanded={false}
        onExpandedChange={onExpandedChange}
      />,
    );
    onExpandedChange.mockClear();
    // Asking again after collapsing is a new request, not an adoption.
    act(() => useAgentChangesState.getState().show(sessionless(), 0));
    expect(onExpandedChange).toHaveBeenCalledWith(true);
  });

  it("holds a Changes request made while the projection loads", () => {
    focusLateChat();
    const onExpandedChange = vi.fn();
    render(
      <AgentArtifactPane
        expanded={false}
        onExpandedChange={onExpandedChange}
      />,
    );
    act(() =>
      useAgentChangesState.getState().show(
        {
          projectId: "project-a",
          chatId: "chat-late",
          sessionId: "session-a",
        },
        0,
      ),
    );
    expect(onExpandedChange).not.toHaveBeenCalledWith(true);
    act(() => {
      acpProjectionStoreFor("chat-late")
        .getState()
        .applyAuthoritativePage(lateScope, pageFor("session-a"));
    });
    expect(onExpandedChange).toHaveBeenCalledWith(true);
  });

  it("does not reuse a sessionless tab while the projection is unloaded", () => {
    focusLateChat();
    act(() =>
      useAgentChangesState.getState().show(
        {
          projectId: "project-a",
          chatId: "chat-late",
          sessionId: null,
        },
        0,
      ),
    );
    render(<AgentArtifactPane expanded />);
    expect(screen.queryByRole("region", { name: "Chat changes" })).toBeNull();
    expect(
      screen.getByRole("heading", { name: "Waiting for this Chat" }),
    ).toBeVisible();
  });

  it("opens an empty review once the projection confirms no session", async () => {
    focusLateChat();
    act(() => {
      acpProjectionStoreFor("chat-late")
        .getState()
        .applyAuthoritativePage(lateScope, pageFor(null));
    });
    const { rerender } = render(<AgentArtifactPane expanded={false} />);
    rerender(<AgentArtifactPane expanded />);
    expect(useAgentChangesState.getState().owner).toEqual({
      projectId: "project-a",
      chatId: "chat-late",
      sessionId: null,
    });
    expect(
      await screen.findByRole("region", { name: "Chat changes" }),
    ).toBeVisible();
    expect(
      screen.queryByRole("heading", { name: "Waiting for this Chat" }),
    ).toBeNull();
  });
});
