import type { WorkbenchTab } from "../domain/workbench";

export function positionTab(tab: WorkbenchTab, position: number): WorkbenchTab {
  return { ...tab, position };
}

export function orderedTabs(tabs: WorkbenchTab[]): WorkbenchTab[] {
  return [...tabs].sort((left, right) => left.position - right.position);
}

export function closeTab(
  tabs: WorkbenchTab[],
  activeTabId: string,
  closingTabId: string,
): { tabs: WorkbenchTab[]; activeTabId: string | null } {
  const ordered = orderedTabs(tabs);
  const closingIndex = ordered.findIndex((tab) => tab.id === closingTabId);
  const closingTab = ordered[closingIndex];

  if (!closingTab || closingTab.dirty) {
    return { tabs, activeTabId };
  }

  return {
    tabs: ordered.filter((tab) => tab.id !== closingTabId).map(positionTab),
    activeTabId:
      activeTabId === closingTabId
        ? (ordered[closingIndex + 1]?.id ??
          ordered[closingIndex - 1]?.id ??
          null)
        : activeTabId,
  };
}
