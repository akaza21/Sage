import type { SettingsPageId } from "../../domain/settingsNavigation";
import { requestSettingsPage } from "../../app/settingsPageRequest";
import type { StoreApi } from "zustand";
import type { WorkbenchState } from "../../app/store";
import { errorMessage } from "../../domain/errors";
import { isCompactWorkbenchViewport } from "../../workbench/workbenchViewport";

export default async function openEditorSettings(
  set: StoreApi<WorkbenchState>["setState"],
  get: StoreApi<WorkbenchState>["getState"],
  source: WorkbenchState,
  rememberReturnTab: (tabId: string | null) => void,
  page?: SettingsPageId,
) {
  const id = "settings:appearance";
  const sourceTab = source.tabs.find((tab) => tab.id === source.activeTabId);
  const isCurrent = (current = get()) =>
    current.projectGeneration === source.projectGeneration &&
    current.project?.id === source.project?.id &&
    current.activeTabId === source.activeTabId;
  if (!isCurrent()) return;
  const activating = sourceTab?.id !== id;
  if (activating) {
    try {
      await source.flushActiveEditor();
    } catch (error) {
      if (isCurrent()) set({ error: errorMessage(error) });
      return;
    }
    const current = get();
    if (
      !isCurrent(current) ||
      current.projectTransitioning ||
      current.fileRenameInFlight ||
      current.resourceTrashInFlight
    )
      return;
    rememberReturnTab(
      sourceTab?.kind === "file" || sourceTab?.kind === "diff"
        ? sourceTab.id
        : null,
    );
    set({
      tabs: current.tabs.some((tab) => tab.id === id)
        ? current.tabs
        : [
            ...current.tabs,
            {
              id,
              kind: "settings",
              title: "Settings",
              position: current.tabs.length,
              pinned: false,
              dirty: false,
            },
          ],
      activeTabId: id,
      activeEditor: null,
    });
  }
  if (isCompactWorkbenchViewport()) set({ sidebarOpen: false });
  requestSettingsPage(page);
  if (activating)
    void get()
      .persistLayout()
      .catch((error) => set({ error: errorMessage(error) }));
}
