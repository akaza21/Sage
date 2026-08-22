import type { TabKind } from "../domain/chat";

export const WORKBENCH_WINDOW_MODES = ["agent", "editor"] as const;

export type WorkbenchWindowMode = (typeof WORKBENCH_WINDOW_MODES)[number];

export const DEFAULT_WORKBENCH_WINDOW_MODE: WorkbenchWindowMode = "editor";

export const AGENT_CENTER_CHAT_SCAN_LIMIT = 64;

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

export type AgentCenterDecision =
  | { action: "none" }
  | { action: "activate"; tabId: string }
  | { action: "create" };

export function agentCenterChatAction(
  tabs: ReadonlyArray<AgentCenterScanTab>,
  activeTabId: string | null,
): AgentCenterDecision {
  const scanLimit = Math.min(tabs.length, AGENT_CENTER_CHAT_SCAN_LIMIT);
  let chatTabId: string | null = null;
  for (let index = 0; index < scanLimit; index += 1) {
    const tab = tabs[index];
    if (tab.kind === "chat") {
      if (tab.id === activeTabId) return { action: "none" };
      if (chatTabId === null) chatTabId = tab.id;
    }
  }
  return chatTabId === null
    ? { action: "create" }
    : { action: "activate", tabId: chatTabId };
}

export function isCompactDrawerOpen(
  compact: boolean,
  sidebarOpen: boolean,
  mode: WorkbenchWindowMode,
): boolean {
  return compact && sidebarOpen && mode === "editor";
}
