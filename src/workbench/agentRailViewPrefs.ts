import { useSyncExternalStore } from "react";

import {
  AGENT_RAIL_SHOW_FIELDS,
  type AgentRailGrouping,
  type AgentRailOrdering,
  type AgentRailShowField,
  type AgentRailStatusFilter,
} from "./agentRailStatusView";

export interface AgentRailViewPrefs {
  grouping: AgentRailGrouping;
  ordering: AgentRailOrdering;
  show: Record<AgentRailShowField, boolean>;
  statusFilter: AgentRailStatusFilter;
  showArchived: boolean;
}

const listeners = new Set<() => void>();
const collapsed = new Set<string>();
let clockMs = 0;

let prefs: AgentRailViewPrefs = {
  grouping: "folder",
  ordering: "recency",
  show: { updated: true, workspace: true, branch: true },
  statusFilter: "all",
  showArchived: true,
};

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
  prefs = {
    grouping: "folder",
    ordering: "recency",
    show: { updated: true, workspace: true, branch: true },
    statusFilter: "all",
    showArchived: true,
  };
  collapsed.clear();
  clockMs = 0;
  emit();
}

export function agentRailClockMs(): number {
  return clockMs;
}

export function syncAgentRailClock(nowMs: number) {
  if (!Number.isFinite(nowMs) || nowMs <= clockMs) return;
  clockMs = nowMs;
  prefs = { ...prefs };
  emit();
}

export function setAgentRailGrouping(grouping: AgentRailGrouping) {
  if (prefs.grouping === grouping) return;
  prefs = { ...prefs, grouping };
  emit();
}

export function setAgentRailOrdering(ordering: AgentRailOrdering) {
  if (prefs.ordering === ordering) return;
  prefs = { ...prefs, ordering };
  emit();
}

export function setAgentRailShowField(
  field: AgentRailShowField,
  value: boolean,
) {
  if (!AGENT_RAIL_SHOW_FIELDS.includes(field) || prefs.show[field] === value) {
    return;
  }
  prefs = { ...prefs, show: { ...prefs.show, [field]: value } };
  emit();
}

export function setAgentRailStatusFilter(statusFilter: AgentRailStatusFilter) {
  if (prefs.statusFilter === statusFilter) return;
  prefs = { ...prefs, statusFilter };
  emit();
}

export function setAgentRailShowArchived(showArchived: boolean) {
  if (prefs.showArchived === showArchived) return;
  prefs = { ...prefs, showArchived };
  emit();
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

export function collapseAllAgentRailSections(ids: ReadonlyArray<string>) {
  for (const id of ids) collapsed.add(id);
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
