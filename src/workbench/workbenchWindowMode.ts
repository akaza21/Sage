import type { TabKind } from "../domain/chat";

export const WORKBENCH_WINDOW_MODES = ["agent", "editor"] as const;

export type WorkbenchWindowMode = (typeof WORKBENCH_WINDOW_MODES)[number];

export const DEFAULT_WORKBENCH_WINDOW_MODE: WorkbenchWindowMode = "editor";

export const AGENT_CENTER_CHAT_SCAN_LIMIT = 64;

export const MAX_AGENT_VISIBLE_CHATS = 2;

export const EDITOR_CANVAS_KINDS = ["file", "diff", "settings"] as const;

export interface AgentCenterScanTab {
  id: string;
  kind: TabKind;
}

const workbenchWindowModes = new Set<string>(WORKBENCH_WINDOW_MODES);

export function isWorkbenchWindowMode(
  value: unknown,
): value is WorkbenchWindowMode {
  return typeof value === "string" && workbenchWindowModes.has(value);
}

export function isAgentArtifactTabKind(kind: TabKind): kind is "file" | "diff" {
  return kind === "file" || kind === "diff";
}

export type AgentCenterDecision = { action: "none" } | { action: "create" };

export function agentCenterChatAction(
  tabs: ReadonlyArray<AgentCenterScanTab>,
  _activeTabId: string | null,
): AgentCenterDecision {
  const scanLimit = Math.min(tabs.length, AGENT_CENTER_CHAT_SCAN_LIMIT);
  for (let index = 0; index < scanLimit; index += 1) {
    if (tabs[index]?.kind === "chat") {
      return { action: "none" };
    }
  }
  return { action: "create" };
}

export function firstChatTabId(
  tabs: ReadonlyArray<AgentCenterScanTab>,
): string | null {
  const ids = collectChatTabIds(tabs);
  return ids[0] ?? null;
}

export function collectChatTabIds(
  tabs: ReadonlyArray<AgentCenterScanTab>,
): string[] {
  const ids: string[] = [];
  const scanLimit = Math.min(tabs.length, AGENT_CENTER_CHAT_SCAN_LIMIT);
  for (let index = 0; index < scanLimit; index += 1) {
    const tab = tabs[index];
    if (tab?.kind === "chat") {
      ids.push(tab.id);
    }
  }
  return ids;
}

function listHasId(ids: readonly string[], candidate: string): boolean {
  const scanLimit = Math.min(ids.length, AGENT_CENTER_CHAT_SCAN_LIMIT);
  for (let index = 0; index < scanLimit; index += 1) {
    if (ids[index] === candidate) return true;
  }
  return false;
}

function pushUniqueId(ids: string[], candidate: string): void {
  if (!listHasId(ids, candidate) && ids.length < MAX_AGENT_VISIBLE_CHATS) {
    ids.push(candidate);
  }
}

export function reconcileVisibleChatIds(
  chatIds: readonly string[],
  visibleIds: readonly string[],
  focusedId: string | null,
): string[] {
  const next: string[] = [];
  const visibleLimit = Math.min(visibleIds.length, MAX_AGENT_VISIBLE_CHATS);
  for (let index = 0; index < visibleLimit; index += 1) {
    const id = visibleIds[index];
    if (id && listHasId(chatIds, id)) {
      pushUniqueId(next, id);
    }
  }
  if (
    focusedId &&
    listHasId(chatIds, focusedId) &&
    !listHasId(next, focusedId)
  ) {
    if (next.length < MAX_AGENT_VISIBLE_CHATS) {
      next.push(focusedId);
    } else {
      next[next.length - 1] = focusedId;
    }
  }
  if (next.length === 0) {
    const seedLimit = Math.min(chatIds.length, MAX_AGENT_VISIBLE_CHATS);
    for (let index = 0; index < seedLimit; index += 1) {
      const id = chatIds[index];
      if (id) next.push(id);
    }
  }
  return next;
}

export function selectVisibleChatIds(
  chatIds: readonly string[],
  visibleIds: readonly string[],
  selectedId: string,
): string[] {
  if (!listHasId(chatIds, selectedId)) {
    return reconcileVisibleChatIds(chatIds, visibleIds, null);
  }
  const kept: string[] = [];
  const visibleLimit = Math.min(visibleIds.length, MAX_AGENT_VISIBLE_CHATS);
  for (let index = 0; index < visibleLimit; index += 1) {
    const id = visibleIds[index];
    if (id && listHasId(chatIds, id)) {
      pushUniqueId(kept, id);
    }
  }
  if (!listHasId(kept, selectedId)) {
    if (kept.length < MAX_AGENT_VISIBLE_CHATS) {
      kept.push(selectedId);
    } else {
      const newest = kept[kept.length - 1];
      return newest ? [newest, selectedId] : [selectedId];
    }
  }
  const fillLimit = Math.min(chatIds.length, AGENT_CENTER_CHAT_SCAN_LIMIT);
  for (let index = 0; index < fillLimit; index += 1) {
    const id = chatIds[index];
    if (id) pushUniqueId(kept, id);
  }
  return kept;
}

export function focusedAgentChatId(
  chatIds: readonly string[],
  visibleIds: readonly string[],
  activeTabId: string | null,
  previousFocusedId: string | null = null,
): string | null {
  if (activeTabId && listHasId(chatIds, activeTabId)) {
    return activeTabId;
  }
  if (
    previousFocusedId &&
    listHasId(chatIds, previousFocusedId) &&
    listHasId(visibleIds, previousFocusedId)
  ) {
    return previousFocusedId;
  }
  return visibleIds[0] ?? chatIds[0] ?? null;
}

export function lastArtifactTabId(
  tabs: ReadonlyArray<AgentCenterScanTab>,
  preferredId: string | null,
): string | null {
  const scanLimit = Math.min(tabs.length, AGENT_CENTER_CHAT_SCAN_LIMIT);
  if (preferredId) {
    for (let index = 0; index < scanLimit; index += 1) {
      const tab = tabs[index];
      if (tab?.id === preferredId && isAgentArtifactTabKind(tab.kind)) {
        return preferredId;
      }
    }
  }
  let found: string | null = null;
  for (let index = 0; index < scanLimit; index += 1) {
    const tab = tabs[index];
    if (tab && isAgentArtifactTabKind(tab.kind)) {
      found = tab.id;
    }
  }
  return found;
}

export function isCompactDrawerOpen(
  compact: boolean,
  sidebarOpen: boolean,
  mode: WorkbenchWindowMode,
): boolean {
  return compact && sidebarOpen && mode === "editor";
}
