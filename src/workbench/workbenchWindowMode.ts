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

const workbenchWindowModes = new Set<string>(WORKBENCH_WINDOW_MODES);

export function isWorkbenchWindowMode(
  value: unknown,
): value is WorkbenchWindowMode {
  return typeof value === "string" && workbenchWindowModes.has(value);
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
  const ids = collectChatTabIds(tabs, null, null, 1);
  return ids[0] ?? null;
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
  return (
    [activeTabId, previousFocusedId, candidates[0]].find((id): id is string =>
      Boolean(id && candidates.includes(id)),
    ) ?? null
  );
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
