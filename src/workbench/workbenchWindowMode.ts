export const WORKBENCH_WINDOW_MODES = ["agent", "editor"] as const;

export type WorkbenchWindowMode = (typeof WORKBENCH_WINDOW_MODES)[number];

export const DEFAULT_WORKBENCH_WINDOW_MODE: WorkbenchWindowMode = "editor";

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
  tabs: ReadonlyArray<{ id: string; kind: string }>,
  activeTabId: string | null,
): AgentCenterDecision {
  const activeTab = tabs.find((tab) => tab.id === activeTabId);
  if (activeTab?.kind === "chat") {
    return { action: "none" };
  }
  const chatTab = tabs.find((tab) => tab.kind === "chat");
  return chatTab
    ? { action: "activate", tabId: chatTab.id }
    : { action: "create" };
}

export function isCompactDrawerOpen(
  compact: boolean,
  sidebarOpen: boolean,
  mode: WorkbenchWindowMode,
): boolean {
  return compact && sidebarOpen && mode === "editor";
}
