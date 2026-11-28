import { useEffect, useRef } from "react";
import { create } from "zustand";
import { useWorkbenchStore } from "../app/store";
import { useSubagentPaneStore } from "../app/subagentPaneStore";
import { useAgentChangesState } from "./agentChangesState";

/**
 * Right-pane views that are not workbench tabs: a Chat's diagrams and, once
 * the backend reports them, a subagent's transcript and a background task's
 * output. They are disposable WebView state derived from the transcript, so
 * they stay bounded and are never persisted.
 */
export type AgentPaneViewKind = "diagram" | "subagentTranscript" | "backgroundTask";
export type AgentPaneView = {
  id: string;
  chatId: string;
  kind: AgentPaneViewKind;
  title: string;
  /** Diagram source; transcript and task views carry their key in `id`. */
  source?: string;
};

export const MAX_PANE_VIEWS_PER_CHAT = 8;
const MAX_PANE_VIEWS = 32;
const launcherId = (chatId: string) => `${chatId}\u0000launcher`;

type PaneViews = {
  views: readonly AgentPaneView[];
  /** A view id, a Chat's launcher id, or null for the pane's own choice. */
  selected: string | null;
  opened: number;
};

export const useAgentPaneViews = create<PaneViews>(() => ({
  views: [],
  selected: null,
  opened: 0,
}));

function deselectOthers() {
  useAgentChangesState.getState().select(false);
  useSubagentPaneStore.getState().select(false);
}

export function selectAgentPaneView(id: string | null) {
  if (id) deselectOthers();
  useAgentPaneViews.setState({ selected: id });
}

export function showAgentPaneLauncher(chatId: string) {
  selectAgentPaneView(launcherId(chatId));
}

export function openAgentPaneView(
  view: Omit<AgentPaneView, "id">,
  key: string,
) {
  const id = `${view.chatId}\u0000${view.kind}\u0000${key}`;
  useAgentPaneViews.setState((state) => {
    const kept = state.views.filter((entry) => entry.id !== id);
    const own = kept.filter((entry) => entry.chatId === view.chatId);
    const evicted =
      own.length >= MAX_PANE_VIEWS_PER_CHAT
        ? own[0].id
        : kept.length >= MAX_PANE_VIEWS
          ? kept[0].id
          : null;
    return {
      views: [...kept.filter((entry) => entry.id !== evicted), { ...view, id }],
      opened: state.opened + 1,
    };
  });
  selectAgentPaneView(id);
}

export function closeAgentPaneView(id: string) {
  useAgentPaneViews.setState((state) => ({
    views: state.views.filter((view) => view.id !== id),
    selected: state.selected === id ? null : state.selected,
  }));
}

/**
 * The focused Chat's views and its selected view or launcher. Choosing a
 * file, diff, browser, Changes or Subagents elsewhere hands the canvas back.
 */
export function useAgentPaneViewsFor(
  chatId: string | null,
  expanded: boolean,
  onExpandedChange?: (expanded: boolean) => void,
) {
  const all = useAgentPaneViews((state) => state.views);
  const selectedId = useAgentPaneViews((state) => state.selected);
  const opened = useAgentPaneViews((state) => state.opened);
  const views = all.filter((view) => view.chatId === chatId);
  const launcher = chatId !== null && selectedId === launcherId(chatId);
  const selected = views.find((view) => view.id === selectedId) ?? null;
  const seen = useRef(opened);
  useEffect(() => {
    if (opened === seen.current) return;
    seen.current = opened;
    if (!expanded && selected) onExpandedChange?.(true);
  }, [expanded, onExpandedChange, opened, selected]);
  useEffect(() => {
    const release = () => useAgentPaneViews.setState({ selected: null });
    const unsubscribers = [
      useAgentChangesState.subscribe((state, previous) => {
        if (state.selected && !previous.selected) release();
      }),
      useSubagentPaneStore.subscribe((state, previous) => {
        if (state.selected && !previous.selected) release();
      }),
      useWorkbenchStore.subscribe((state, previous) => {
        if (state.activeTabId === previous.activeTabId) return;
        const tab = state.tabs.find(({ id }) => id === state.activeTabId);
        if (tab && tab.kind !== "chat") release();
      }),
    ];
    return () => unsubscribers.forEach((unsubscribe) => unsubscribe());
  }, []);
  return { views, selected, launcher };
}
