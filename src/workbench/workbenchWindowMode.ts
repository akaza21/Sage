import type { TabKind } from "../domain/chat";

export const WORKBENCH_WINDOW_MODES = ["agent", "editor"] as const;

export type WorkbenchWindowMode = (typeof WORKBENCH_WINDOW_MODES)[number];

export const DEFAULT_WORKBENCH_WINDOW_MODE: WorkbenchWindowMode = "agent";

export const AGENT_CENTER_CHAT_SCAN_LIMIT = 64;

export const EDITOR_CANVAS_KINDS = ["file", "diff", "settings"] as const;

export interface AgentCenterScanTab {
  chatListed?: boolean;
  id: string;
  kind: TabKind;
}

export function isWorkbenchWindowMode(
  value: unknown,
): value is WorkbenchWindowMode {
  return WORKBENCH_WINDOW_MODES.includes(value as WorkbenchWindowMode);
}

export function isAgentArtifactTabKind(kind: TabKind): kind is "file" | "diff" {
  return kind === "file" || kind === "diff";
}

export function agentCenterNeedsDraft(
  tabs: ReadonlyArray<AgentCenterScanTab>,
  activeTabId: string | null,
  focusedChatId?: string | null,
): boolean {
  return !collectChatTabIds(tabs, activeTabId, focusedChatId, 1).length;
}

export function firstChatTabId(
  tabs: ReadonlyArray<AgentCenterScanTab>,
): string | null {
  return collectChatTabIds(tabs, null, null, 1)[0] ?? null;
}

export function collectChatTabIds(
  tabs: ReadonlyArray<AgentCenterScanTab>,
  activeTabId: string | null = null,
  focusedChatId: string | null = null,
  limit = AGENT_CENTER_CHAT_SCAN_LIMIT,
): string[] {
  const ids: string[] = [];
  const scanLimit = Math.min(tabs.length, AGENT_CENTER_CHAT_SCAN_LIMIT);
  for (let index = 0; index < scanLimit; index += 1) {
    const tab = tabs[index];
    if (
      tab?.kind === "chat" &&
      (tab.chatListed !== false ||
        tab.id === activeTabId ||
        tab.id === focusedChatId)
    ) {
      ids.push(tab.id);
      if (ids.length >= limit) break;
    }
  }
  return ids;
}

export function focusedAgentChatId(
  candidates: readonly string[],
  activeTabId: string | null,
  previousFocusedId: string | null,
): string | null {
  if (activeTabId && candidates.includes(activeTabId)) return activeTabId;
  if (previousFocusedId && candidates.includes(previousFocusedId))
    return previousFocusedId;
  return candidates[0] || null;
}

export function lastArtifactTabId(
  tabs: ReadonlyArray<AgentCenterScanTab>,
  preferredId: string | null,
): string | null {
  const scanLimit = Math.min(tabs.length, AGENT_CENTER_CHAT_SCAN_LIMIT);
  let found: string | null = null;
  for (let index = 0; index < scanLimit; index += 1) {
    const tab = tabs[index];
    if (tab && isAgentArtifactTabKind(tab.kind)) {
      if (preferredId && tab.id === preferredId) return tab.id;
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
