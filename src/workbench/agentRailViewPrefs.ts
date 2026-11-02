import { useSyncExternalStore } from "react";

import {
  AGENT_RAIL_ORDERINGS,
  AGENT_RAIL_ORGANIZE_MODES,
  type AgentRailOrdering,
  type AgentRailOrganizeMode,
} from "./agentRailStatusView";

export interface AgentRailViewPrefs {
  organize: AgentRailOrganizeMode;
  ordering: AgentRailOrdering;
}

/** Disposable view state: how this WebView lays out the rail. */
export const AGENT_RAIL_VIEW_STORAGE_KEY = "sage.agent-rail.view.v1";

const DEFAULT_PREFS: AgentRailViewPrefs = {
  organize: "project",
  ordering: "recency",
};

const listeners = new Set<() => void>();
const collapsed = new Set<string>();

let prefs: AgentRailViewPrefs = readStoredPrefs();

function oneOf<T extends string>(
  values: ReadonlyArray<T>,
  value: unknown,
): value is T {
  return values.some((candidate) => candidate === value);
}

function readStoredPrefs(): AgentRailViewPrefs {
  try {
    const stored = localStorage.getItem(AGENT_RAIL_VIEW_STORAGE_KEY);
    if (stored === null) return DEFAULT_PREFS;
    const parsed: unknown = JSON.parse(stored);
    if (typeof parsed !== "object" || parsed === null) return DEFAULT_PREFS;
    const { organize, ordering } = parsed as Record<string, unknown>;
    return {
      organize: oneOf(AGENT_RAIL_ORGANIZE_MODES, organize)
        ? organize
        : DEFAULT_PREFS.organize,
      ordering: oneOf(AGENT_RAIL_ORDERINGS, ordering)
        ? ordering
        : DEFAULT_PREFS.ordering,
    };
  } catch {
    return DEFAULT_PREFS;
  }
}

function update(next: AgentRailViewPrefs) {
  prefs = next;
  try {
    localStorage.setItem(AGENT_RAIL_VIEW_STORAGE_KEY, JSON.stringify(next));
  } catch {
    // Private or quota-limited WebViews keep the choice for this session.
  }
  emit();
}

function emit() {
  for (const listener of listeners) listener();
}

export function subscribeAgentRailViewPrefs(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function agentRailViewPrefs(): AgentRailViewPrefs {
  return prefs;
}

export function resetAgentRailViewPrefs() {
  prefs = DEFAULT_PREFS;
  collapsed.clear();
  emit();
}

export function setAgentRailOrganize(organize: AgentRailOrganizeMode) {
  if (prefs.organize === organize) return;
  update({ ...prefs, organize });
}

export function setAgentRailOrdering(ordering: AgentRailOrdering) {
  if (prefs.ordering === ordering) return;
  update({ ...prefs, ordering });
}

export function agentRailSectionCollapsed(id: string): boolean {
  return collapsed.has(id);
}

export function toggleAgentRailSection(id: string) {
  if (collapsed.has(id)) collapsed.delete(id);
  else collapsed.add(id);
  prefs = { ...prefs };
  emit();
}

export function expandAgentRailSection(id: string) {
  if (!collapsed.delete(id)) return;
  prefs = { ...prefs };
  emit();
}

export function useAgentRailViewPrefs(): AgentRailViewPrefs {
  return useSyncExternalStore(
    subscribeAgentRailViewPrefs,
    agentRailViewPrefs,
    agentRailViewPrefs,
  );
}
