import type { WorkbenchTab } from "../domain/workbench";

export function orderedTabs(tabs: WorkbenchTab[]): WorkbenchTab[] {
  return [...tabs].sort((left, right) => left.position - right.position);
}

export function nextActiveTabId(
  tabs: WorkbenchTab[],
  activeTabId: string,
  closingTabId: string,
): string | null {
  const ordered = orderedTabs(tabs);
  const closingIndex = ordered.findIndex((tab) => tab.id === closingTabId);

  if (closingIndex === -1) {
    return activeTabId;
  }

  if (activeTabId !== closingTabId) {
    return activeTabId;
  }

  return ordered[closingIndex + 1]?.id ?? ordered[closingIndex - 1]?.id ?? null;
}

export function closeTab(
  tabs: WorkbenchTab[],
  activeTabId: string,
  closingTabId: string,
): { tabs: WorkbenchTab[]; activeTabId: string | null } {
  const closingTab = tabs.find((tab) => tab.id === closingTabId);

  if (!closingTab || closingTab.dirty) {
    return { tabs, activeTabId };
  }

  return {
    tabs: tabs.filter((tab) => tab.id !== closingTabId),
    activeTabId: nextActiveTabId(tabs, activeTabId, closingTabId),
  };
}
