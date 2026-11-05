import {
  pinAgentChatOwner,
  pinnedAgentChatOwner,
  clearAgentChatOwner,
  releaseAgentChatOwner,
  useAgentChatFocus,
} from "../app/agentChatOwner";
import "./navigationPaneHistory";
import type { AgentArtifactOwner } from "../domain/agentArtifact";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useAgentArtifactFollowStore } from "../app/agentArtifactFollowStore";
import { useAgentChangesState } from "./agentChangesState";
import { useSubagentPaneStore } from "../app/subagentPaneStore";
import { useWorkbenchStore } from "../app/store";
import { resetBrowserDesktopForTests } from "../services/browserDesktop";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import { keymapBinding, formatKeymapChords } from "../domain/keymapRegistry";
import { useWorkbenchKeys } from "./useWorkbenchKeys";
import { useEditorHistoryKeys } from "./useEditorHistoryKeys";
import {
  connectNavigationHistory,
  navigateWorkbenchBack,
  navigateWorkbenchForward,
  navigationHistory,
} from "./navigationHistory";
import type { WorkbenchWindowMode } from "./workbenchWindowMode";

const initial = useWorkbenchStore.getState();
let connection: ReturnType<typeof connectNavigationHistory> | undefined;
beforeEach(async () => {
  localStorage.clear();
  resetBrowserDesktopForTests();
  useWorkbenchStore.setState(initial, true);
  useAgentSettingsState.getState().close();
  await useWorkbenchStore.getState().initialize();
});
afterEach(() => {
  cleanup();
  clearAgentChatOwner();
  useAgentArtifactFollowStore.getState().clear();
  useAgentChangesState.getState().close();
  useSubagentPaneStore.getState().clear();
  connection?.dispose();
  connection = undefined;
  useWorkbenchStore.setState(initial, true);
  resetBrowserDesktopForTests();
  vi.restoreAllMocks();
});
function connect() {
  let mode: WorkbenchWindowMode = "agent";
  connection = connectNavigationHistory(
    () => mode,
    (next) => {
      mode = next;
    },
  );
}
function press(
  key: string,
  extra: KeyboardEventInit = {},
  target: EventTarget = window,
) {
  const event = new KeyboardEvent("keydown", {
    key,
    altKey: true,
    bubbles: true,
    cancelable: true,
    ...extra,
  });
  act(() => target.dispatchEvent(event));
  return event;
}

it("navigates real chat activations, Settings open/close and tool panes", async () => {
  connect();
  const first = useWorkbenchStore.getState().activeTabId;
  await useWorkbenchStore.getState().createChat();
  const second = useWorkbenchStore.getState().activeTabId;
  expect(second).not.toBe(first);
  useAgentSettingsState.getState().open("Keybindings");
  await Promise.resolve();
  useAgentSettingsState.getState().close();
  await Promise.resolve();
  useWorkbenchStore.getState().setSidebarView("search");
  await navigateWorkbenchBack();
  expect(useWorkbenchStore.getState().sidebarView).toBe("explorer");
  await navigateWorkbenchBack();
  expect(useAgentSettingsState.getState().page).toBe("Keybindings");
  await navigateWorkbenchBack();
  expect(useAgentSettingsState.getState().page).toBeNull();
  await navigateWorkbenchBack();
  expect(useWorkbenchStore.getState().activeTabId).toBe(first);
  await navigateWorkbenchForward();
  expect(useWorkbenchStore.getState().activeTabId).toBe(second);
});

it.each(["deleted", "archived"])(
  "skips a %s chat when traversing either direction",
  async (kind) => {
    connect();
    const first = useWorkbenchStore.getState().activeTabId;
    await useWorkbenchStore.getState().createChat();
    const removed = useWorkbenchStore.getState().activeTabId!;
    await useWorkbenchStore.getState().createChat();
    const third = useWorkbenchStore.getState().activeTabId;
    // Apply the authoritative projection received after deletion/archival.
    useWorkbenchStore.setState((state) => ({
      tabs: state.tabs.filter((tab) => tab.id !== removed),
      archivedChats:
        kind === "archived" ? [{ id: removed, title: "Archived" }] : [],
    }));
    await navigateWorkbenchBack();
    expect(useWorkbenchStore.getState().activeTabId).toBe(first);
    await navigateWorkbenchForward();
    expect(useWorkbenchStore.getState().activeTabId).toBe(third);
    expect(
      navigationHistory
        .getState()
        .entries.some((entry) => entry.tabId === removed),
    ).toBe(false);
  },
);

it("keeps history on a refused activation and accepts a later retry", async () => {
  connect();
  const first = useWorkbenchStore.getState().activeTabId;
  await useWorkbenchStore.getState().createChat();
  const index = navigationHistory.getState().index;
  useWorkbenchStore.setState({ fileRenameInFlight: true });
  await navigateWorkbenchBack();
  expect(navigationHistory.getState().index).toBe(index);
  useWorkbenchStore.setState({ fileRenameInFlight: false });
  await navigateWorkbenchBack();
  expect(useWorkbenchStore.getState().activeTabId).toBe(first);
});

it("registers Alt+Left/Right on both platforms and runs each command once", async () => {
  const { unmount } = renderHook(() => {
    const [windowMode, setWindowMode] = useState<WorkbenchWindowMode>("editor");
    useWorkbenchKeys({
      resourceModal: false,
      windowMode,
      setWindowMode,
      toggleAgentNavigation: () => {},
    });
    useEditorHistoryKeys(true);
  });
  for (const apple of [false, true]) {
    expect(formatKeymapChords(keymapBinding("workbenchBack"), apple)).toEqual([
      apple ? "Option+Left" : "Alt+Left",
    ]);
  }
  await waitFor(() =>
    expect(navigationHistory.getState().entries.length).toBeGreaterThan(0),
  );
  const first = useWorkbenchStore.getState().activeTabId;
  await act(() => useWorkbenchStore.getState().createChat());
  const second = useWorkbenchStore.getState().activeTabId;
  expect(press("ArrowLeft").defaultPrevented).toBe(true);
  await waitFor(() =>
    expect(useWorkbenchStore.getState().activeTabId).toBe(first),
  );
  press("ArrowRight");
  await waitFor(() =>
    expect(useWorkbenchStore.getState().activeTabId).toBe(second),
  );
  expect(press("ArrowLeft", { shiftKey: true }).defaultPrevented).toBe(false);
  expect(press("ArrowLeft", { isComposing: true }).defaultPrevented).toBe(
    false,
  );
  unmount();
  expect(navigationHistory.getState().entries).toEqual([]);
  await useWorkbenchStore.getState().createChat();
  useAgentSettingsState.getState().open();
  expect(navigationHistory.getState().entries).toEqual([]);
  expect(press("ArrowLeft").defaultPrevented).toBe(false);
});

it("does not steal text-editing chords or navigate through a resource modal", async () => {
  const { rerender } = renderHook(
    ({ resourceModal }) => {
      const [windowMode, setWindowMode] =
        useState<WorkbenchWindowMode>("agent");
      useWorkbenchKeys({
        resourceModal,
        windowMode,
        setWindowMode,
        toggleAgentNavigation: () => {},
      });
    },
    { initialProps: { resourceModal: false } },
  );
  await act(() => useWorkbenchStore.getState().createChat());
  const input = document.createElement("textarea");
  document.body.append(input);
  expect(press("ArrowLeft", {}, input).defaultPrevented).toBe(false);
  input.remove();
  rerender({ resourceModal: true });
  expect(press("ArrowLeft").defaultPrevented).toBe(false);
});

it("restores Agent Changes, Preview and subagent pane selections using IDs", async () => {
  connect();
  const state = useWorkbenchStore.getState();
  const owner = {
    projectId: state.project!.id,
    chatId: state.activeTabId!,
    sessionId: "session-history",
  } as AgentArtifactOwner;
  useAgentChangesState.getState().show(owner, state.projectGeneration);
  await Promise.resolve();
  useAgentChangesState.getState().select(false);
  useAgentArtifactFollowStore.getState().pin(owner, { kind: "preview" });
  await Promise.resolve();
  useSubagentPaneStore
    .getState()
    .show({ owner, itemId: "item-child", agentKey: "agent-child" });
  await Promise.resolve();
  await navigateWorkbenchBack();
  expect(useSubagentPaneStore.getState().selected).toBe(false);
  expect(useAgentArtifactFollowStore.getState().records[0].selected).toEqual({
    kind: "preview",
  });
  await navigateWorkbenchBack();
  expect(useAgentChangesState.getState().selected).toBe(true);
  await navigateWorkbenchBack();
  expect(useAgentChangesState.getState().selected).toBe(false);
  expect(useAgentArtifactFollowStore.getState().records[0].selected).toBeNull();
  await navigateWorkbenchForward();
  await navigateWorkbenchForward();
  await navigateWorkbenchForward();
  expect(useSubagentPaneStore.getState().open).toEqual({
    owner,
    itemId: "item-child",
    agentKey: "agent-child",
  });
  expect(useSubagentPaneStore.getState().selected).toBe(true);
});

it("restores Editor settings after their tab was closed", async () => {
  connection = connectNavigationHistory(
    () => "editor",
    () => {},
  );
  const first = useWorkbenchStore.getState().activeTabId;
  await useWorkbenchStore.getState().openSettings();
  useWorkbenchStore.getState().closeTab("settings:appearance");
  await waitFor(() => {
    expect(useWorkbenchStore.getState().activeTabId).toBe(first);
    expect(useWorkbenchStore.getState().projectTransitioning).toBe(false);
  });
  await navigateWorkbenchBack();
  expect(useWorkbenchStore.getState().activeTabId).toBe("settings:appearance");
  await navigateWorkbenchForward();
  expect(useWorkbenchStore.getState().activeTabId).toBe(first);
});

it("keeps keyboard ownership after a modal changes the listener order", async () => {
  const { rerender } = renderHook(
    ({ resourceModal }) => {
      const [windowMode, setWindowMode] =
        useState<WorkbenchWindowMode>("editor");
      useWorkbenchKeys({
        resourceModal,
        windowMode,
        setWindowMode,
        toggleAgentNavigation: () => {},
      });
      useEditorHistoryKeys(true);
    },
    { initialProps: { resourceModal: true } },
  );
  await waitFor(() =>
    expect(navigationHistory.getState().entries.length).toBeGreaterThan(0),
  );
  const first = useWorkbenchStore.getState().activeTabId;
  await act(() => useWorkbenchStore.getState().createChat());
  rerender({ resourceModal: false });
  press("ArrowLeft");
  await waitFor(() =>
    expect(useWorkbenchStore.getState().activeTabId).toBe(first),
  );
});

it("travels across projects and prunes a foreign chat archived since the visit", async () => {
  connect();
  const project = useWorkbenchStore.getState().project!;
  const first = useWorkbenchStore.getState().activeTabId;
  await useWorkbenchStore.getState().createChat();
  const archived = useWorkbenchStore.getState().activeTabId!;
  const firstTabs = useWorkbenchStore.getState().tabs;
  const otherProject = { ...project, id: "project-other" };
  const otherTab = { ...firstTabs[0], id: "chat-other" };
  let archivedAway = false;
  // The browser fixture only supports one Project; supply the authoritative
  // project-load boundary while exercising real subscriptions and commands.
  vi.spyOn(useWorkbenchStore.getState(), "switchProject").mockImplementation(
    async (id) => {
      useWorkbenchStore.setState(
        id === project.id
          ? {
              project,
              tabs: firstTabs.filter(
                (tab) => !archivedAway || tab.id !== archived,
              ),
              activeTabId: first,
              archivedChats: archivedAway
                ? [{ id: archived, title: "Archived" }]
                : [],
            }
          : {
              project: otherProject,
              tabs: [otherTab],
              activeTabId: otherTab.id,
              archivedChats: [],
            },
      );
    },
  );
  await useWorkbenchStore.getState().switchProject(otherProject.id);
  archivedAway = true;
  await navigateWorkbenchBack();
  expect(useWorkbenchStore.getState().project?.id).toBe(project.id);
  expect(useWorkbenchStore.getState().activeTabId).toBe(first);
  expect(
    navigationHistory
      .getState()
      .entries.some((entry) => entry.tabId === archived),
  ).toBe(false);
  await navigateWorkbenchForward();
  expect(useWorkbenchStore.getState().project?.id).toBe(otherProject.id);
  expect(useWorkbenchStore.getState().activeTabId).toBe(otherTab.id);
});

it("records window changes and restores the mode with Back and Forward", async () => {
  const { result } = renderHook(() => {
    const [windowMode, setWindowMode] = useState<WorkbenchWindowMode>("agent");
    useWorkbenchKeys({
      resourceModal: false,
      windowMode,
      setWindowMode,
      toggleAgentNavigation: () => {},
    });
    return { windowMode, setWindowMode };
  });
  await waitFor(() =>
    expect(navigationHistory.getState().entries).toHaveLength(1),
  );
  act(() => result.current.setWindowMode("editor"));
  await act(async () => {
    await navigateWorkbenchBack();
  });
  expect(result.current.windowMode).toBe("agent");
  await act(async () => {
    await navigateWorkbenchForward();
  });
  expect(result.current.windowMode).toBe("editor");
});

it("does not retain subscriptions after an immediate unmount", async () => {
  const { unmount } = renderHook(() => {
    const [windowMode, setWindowMode] = useState<WorkbenchWindowMode>("agent");
    useWorkbenchKeys({
      resourceModal: false,
      windowMode,
      setWindowMode,
      toggleAgentNavigation: () => {},
    });
  });
  unmount();
  await Promise.resolve();
  await useWorkbenchStore.getState().createChat();
  expect(navigationHistory.getState().entries).toEqual([]);
});

it("restores the owning chat when returning to its artifact from another chat", async () => {
  connect();
  const state = useWorkbenchStore.getState();
  const owner = {
    projectId: state.project!.id,
    chatId: state.activeTabId!,
    sessionId: "session-history",
  } as AgentArtifactOwner;
  pinAgentChatOwner(owner.chatId);
  await state.openFile({
    rootId: state.project!.roots[0].id,
    relativePath: "src/workbench/Workbench.tsx",
  });
  const tabId = useWorkbenchStore.getState().activeTabId!;
  useAgentArtifactFollowStore.getState().pin(owner, { kind: "tab", tabId });
  await Promise.resolve();
  await useWorkbenchStore.getState().createChat();
  const other = useWorkbenchStore.getState().activeTabId!;
  pinAgentChatOwner(other);
  await navigateWorkbenchBack();
  expect(useWorkbenchStore.getState().activeTabId).toBe(tabId);
  expect(pinnedAgentChatOwner()).toBe(owner.chatId);
});

it("retains the underlying chat when replaying Settings with the Agent pin withdrawn", async () => {
  connect();
  const state = useWorkbenchStore.getState();
  const chatId = state.activeTabId!;
  pinAgentChatOwner(chatId);
  await state.openFile({
    rootId: state.project!.roots[0].id,
    relativePath: "src/workbench/Workbench.tsx",
  });
  useAgentSettingsState.getState().open("Appearance");
  releaseAgentChatOwner();
  await Promise.resolve();
  useAgentSettingsState.getState().close();
  await useWorkbenchStore.getState().createChat();
  pinAgentChatOwner(useWorkbenchStore.getState().activeTabId);
  await navigateWorkbenchBack();
  expect(useAgentSettingsState.getState().page).toBe("Appearance");
  expect(pinnedAgentChatOwner()).toBeNull();
  expect(renderHook(() => useAgentChatFocus()).result.current).toBe(chatId);
});
