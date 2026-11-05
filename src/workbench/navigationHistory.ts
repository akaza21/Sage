import { createStore } from "zustand/vanilla";
import { useWorkbenchStore, type SidebarView } from "../app/store";
import {
  useAgentSettingsState,
  type AgentSettingsPage,
} from "../features/settings/agentSettingsState";
import type { WorkbenchWindowMode } from "./workbenchWindowMode";

/** IDs only: never retain an artifact, projection, turn or transcript. */
export interface NavigationPane {
  kind: "changes" | "tab" | "preview" | "subagent";
  chatId: string;
  sessionId: string | null;
  targetId: string | null;
  agentKey: string | null;
}
interface PaneAdapter {
  capture: () => NavigationPane | null;
  restore: (pane: NavigationPane | null) => void;
  subscribe: (observe: () => void) => () => void;
}
let paneAdapter: PaneAdapter | null = null;
let paneObserver: (() => void) | null = null;
let unsubscribePanes: (() => void) | null = null;
/** Agent shortcut registration supplies its already-loaded pane stores. */
export function registerNavigationPanes(adapter: PaneAdapter) {
  unsubscribePanes?.();
  paneAdapter = adapter;
  unsubscribePanes = paneObserver ? adapter.subscribe(paneObserver) : null;
}
/** Total entries, including the current location and both directions. */
export const MAX_NAVIGATION_ENTRIES = 50;
export interface NavigationEntry {
  projectId: string | null;
  tabId: string | null;
  mode: WorkbenchWindowMode;
  settingsPage: AgentSettingsPage | null;
  sidebarView: SidebarView;
  pane?: NavigationPane | null;
}
interface HistoryState {
  entries: readonly NavigationEntry[];
  index: number;
  record: (entry: NavigationEntry) => void;
  prune: (valid: (entry: NavigationEntry) => boolean) => void;
  step: (direction: -1 | 1) => NavigationEntry | null;
}
const same = (a: NavigationEntry | undefined, b: NavigationEntry) =>
  a?.projectId === b.projectId &&
  a.tabId === b.tabId &&
  a.mode === b.mode &&
  a.settingsPage === b.settingsPage &&
  a.sidebarView === b.sidebarView &&
  a.pane?.kind === b.pane?.kind &&
  a.pane?.chatId === b.pane?.chatId &&
  a.pane?.sessionId === b.pane?.sessionId &&
  a.pane?.targetId === b.pane?.targetId &&
  a.pane?.agentKey === b.pane?.agentKey;

export function createNavigationHistory() {
  return createStore<HistoryState>((set, get) => ({
    entries: [],
    index: -1,
    record: (entry) => {
      const { entries, index } = get();
      if (same(entries[index], entry)) return;
      // Copy only destination fields, never retain an upstream object or content.
      const { projectId, tabId, mode, settingsPage, sidebarView, pane } = entry;
      const next = [
        ...entries.slice(0, index + 1),
        {
          projectId,
          tabId,
          mode,
          settingsPage,
          sidebarView,
          ...(pane
            ? {
                pane: {
                  kind: pane.kind,
                  chatId: pane.chatId,
                  sessionId: pane.sessionId,
                  targetId: pane.targetId,
                  agentKey: pane.agentKey,
                },
              }
            : {}),
        },
      ].slice(-MAX_NAVIGATION_ENTRIES);
      set({ entries: next, index: next.length - 1 });
    },
    prune: (valid) => {
      const state = get();
      let index = -1;
      const entries = state.entries.filter((entry, position) => {
        if (!valid(entry)) return false;
        if (position <= state.index) index++;
        return true;
      });
      if (entries.length !== state.entries.length) set({ entries, index });
    },
    step: (direction) => {
      const { entries, index } = get();
      const next = index + direction;
      if (next < 0 || next >= entries.length) return null;
      set({ index: next });
      return entries[next];
    },
  }));
}

/** Disposable window history; title-bar consumers can subscribe to this store. */
export const navigationHistory = createNavigationHistory();
let travel: ((direction: -1 | 1) => Promise<void>) | null = null;
export const hasWorkbenchNavigation = () => travel !== null;
export const navigateWorkbenchBack = () => travel?.(-1);
export const navigateWorkbenchForward = () => travel?.(1);

/** Own subscriptions for exactly the mounted workbench's lifetime. */
export function connectNavigationHistory(
  getMode: () => WorkbenchWindowMode,
  setMode: (mode: WorkbenchWindowMode) => void,
) {
  let disposed = false;
  let navigating = false;
  const capture = (): NavigationEntry => {
    const state = useWorkbenchStore.getState();
    return {
      projectId: state.project?.id ?? null,
      tabId: state.activeTabId,
      mode: getMode(),
      settingsPage: useAgentSettingsState.getState().page,
      sidebarView: state.sidebarView,
      pane: getMode() === "agent" ? (paneAdapter?.capture() ?? null) : null,
    };
  };
  const valid = (entry: NavigationEntry) => {
    const state = useWorkbenchStore.getState();
    // Only the current Project's authoritative inventory proves deletion.
    if (state.projectTransitioning || entry.projectId !== state.project?.id)
      return true;
    if (
      entry.pane &&
      (!state.tabs.some(
        (tab) => tab.kind === "chat" && tab.id === entry.pane!.chatId,
      ) ||
        state.archivedChats.some((chat) => chat.id === entry.pane!.chatId))
    )
      return false;
    if (
      entry.pane?.kind === "tab" &&
      !state.tabs.some((tab) => tab.id === entry.pane!.targetId)
    )
      return false;
    if (entry.tabId === null || entry.tabId === "settings:appearance")
      return true;
    return (
      !state.archivedChats.some((chat) => chat.id === entry.tabId) &&
      state.tabs.some((tab) => tab.id === entry.tabId)
    );
  };
  const observe = () => {
    if (disposed || navigating) return;
    const state = useWorkbenchStore.getState();
    if (state.phase !== "ready" || state.projectTransitioning) return;
    navigationHistory.getState().prune(valid);
    const entry = capture();
    if (valid(entry)) navigationHistory.getState().record(entry);
  };
  const open = async (entry: NavigationEntry) => {
    let state = useWorkbenchStore.getState();
    if (
      state.projectTransitioning ||
      state.fileRenameInFlight ||
      state.resourceTrashInFlight ||
      state.pendingCloseTabId
    )
      return "blocked";
    if (entry.projectId !== (state.project?.id ?? null)) {
      if (!entry.projectId) return "missing";
      await state.switchProject(entry.projectId);
      if (disposed) return "blocked";
      state = useWorkbenchStore.getState();
      if (state.project?.id !== entry.projectId) return "blocked";
    }
    if (!valid(entry) || (entry.tabId === null && state.activeTabId !== null))
      return "missing";
    if (entry.tabId === "settings:appearance") await state.openSettings();
    else if (entry.tabId) await state.activateTab(entry.tabId);
    if (disposed) return "blocked";
    state = useWorkbenchStore.getState();
    if (state.activeTabId !== entry.tabId) return "blocked";
    if (entry.settingsPage)
      useAgentSettingsState.getState().open(entry.settingsPage);
    else useAgentSettingsState.getState().close();
    // Selecting a tool must not force open a sidebar the user collapsed.
    useWorkbenchStore.setState({ sidebarView: entry.sidebarView });
    if (entry.mode === "agent") paneAdapter?.restore(entry.pane ?? null);
    setMode(entry.mode);
    return "opened";
  };
  const navigate = async (direction: -1 | 1) => {
    if (disposed || navigating) return;
    observe();
    navigationHistory.getState().prune(valid);
    navigating = true;
    try {
      for (let attempts = 0; attempts < MAX_NAVIGATION_ENTRIES; attempts++) {
        const before = navigationHistory.getState();
        const entry = before.step(direction);
        if (!entry) break;
        const outcome = await open(entry).catch(() => "blocked" as const);
        if (disposed) return;
        if (outcome === "opened") break;
        if (outcome === "blocked") {
          navigationHistory.setState({
            entries: before.entries,
            index: before.index,
          });
          break;
        }
        // A foreign Project can reveal a deleted/archived Chat only on arrival.
        navigationHistory.setState({ index: before.index });
        navigationHistory
          .getState()
          .prune((candidate) => candidate !== entry && valid(candidate));
      }
    } finally {
      navigating = false;
      observe();
    }
  };
  travel = navigate;
  let queued = false;
  const schedule = () => {
    if (queued || disposed || navigating) return;
    queued = true;
    queueMicrotask(() => {
      queued = false;
      observe();
    });
  };
  const unsubscribeWorkbench = useWorkbenchStore.subscribe((next, previous) => {
    if (
      next.activeTabId !== previous.activeTabId ||
      next.tabs !== previous.tabs ||
      next.archivedChats !== previous.archivedChats ||
      next.project !== previous.project ||
      next.phase !== previous.phase ||
      next.projectTransitioning !== previous.projectTransitioning ||
      next.sidebarView !== previous.sidebarView
    )
      schedule();
  });
  const unsubscribeSettings = useAgentSettingsState.subscribe(schedule);
  paneObserver = schedule;
  unsubscribePanes = paneAdapter?.subscribe(schedule) ?? null;
  observe();
  return {
    observe,
    dispose: () => {
      disposed = true;
      unsubscribeWorkbench();
      unsubscribeSettings();
      if (travel === navigate) {
        unsubscribePanes?.();
        unsubscribePanes = null;
        paneObserver = null;
        travel = null;
        navigationHistory.setState({ entries: [], index: -1 });
      }
    },
  };
}
