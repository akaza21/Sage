import { useEffect, useRef } from "react";
import { create } from "zustand";
import { useWorkbenchStore } from "../app/store";
import { useSubagentPaneStore } from "../app/subagentPaneStore";
import { useAgentChangesState } from "./agentChangesState";

/**
 * Right-pane views that are not workbench tabs: a Chat's diagrams and, once
 * the backend reports them, a subagent's transcript and a background task's
 * output. They are disposable WebView state derived from the transcript, so
 * they stay bounded (see openAgentPaneView) and are never persisted.
 */
export type AgentPaneViewKind =
  "diagram" | "subagentTranscript" | "backgroundTask";
export type AgentPaneView = {
  id: string;
  chatId: string;
  kind: AgentPaneViewKind;
  title: string;
  /** Diagram source; transcript and task views carry their key in `id`. */
  source?: string;
};

const launcherId = (chatId: string) => `${chatId}\u0000launcher`;

type PaneViews = {
  views: readonly AgentPaneView[];
  /** A view id, a Chat's launcher id, or null for the pane's own choice. */
  selected: string | null;
  /** The workbench tab that was active when `selected` was chosen. */
  tab: string | null;
  opened: number;
};

export const useAgentPaneViews = create<PaneViews>(() => ({
  views: [],
  selected: null,
  tab: null,
  opened: 0,
}));

export function selectAgentPaneView(id: string | null) {
  if (id) {
    useAgentChangesState.getState().select(false);
    useSubagentPaneStore.getState().select(false);
  }
  useAgentPaneViews.setState({
    selected: id,
    tab: useWorkbenchStore.getState().activeTabId,
  });
}

export function showAgentPaneLauncher(chatId: string) {
  selectAgentPaneView(launcherId(chatId));
}

export function closeAgentPaneView(id: string) {
  useAgentPaneViews.setState((state) => ({
    views: state.views.filter((view) => view.id !== id),
    selected: state.selected === id ? null : state.selected,
  }));
}

/**
 * The focused Chat's views and its selected view or launcher. Choosing
 * Changes, Subagents, or a file, diff or browser tab since takes precedence.
 */
export function useAgentPaneViewsFor(
  chatId: string | null,
  expanded: boolean,
  onExpandedChange?: (expanded: boolean) => void,
) {
  const { views: all, selected: selectedId, tab, opened } = useAgentPaneViews();
  const otherTab = useWorkbenchStore(
    ({ activeTabId, tabs }) =>
      activeTabId !== tab &&
      tabs.some(({ id, kind }) => id === activeTabId && kind !== "chat"),
  );
  const changesSelected = useAgentChangesState((state) => state.selected);
  const subagentSelected = useSubagentPaneStore((state) => state.selected);
  const other = changesSelected || subagentSelected || otherTab;
  const views = all.filter((view) => view.chatId === chatId);
  const launcher =
    !other && chatId !== null && selectedId === launcherId(chatId);
  const selected =
    (!other && views.find((view) => view.id === selectedId)) || null;
  const seen = useRef(opened);
  useEffect(() => {
    if (opened === seen.current) return;
    seen.current = opened;
    if (!expanded && selected) onExpandedChange?.(true);
  }, [expanded, onExpandedChange, opened, selected]);
  return { views, selected, launcher };
}
