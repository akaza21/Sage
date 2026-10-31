import { sameResource } from "../domain/resourceIdentity";
import type { ResourceRef, WorkbenchTab } from "../domain/workbench";

export const UNSAVED_FILE_REASON =
  "Save or revert this File before renaming or recycling it.";

/** File and Diff tabs that show exactly this File. */
export function tabsShowing(
  tabs: readonly WorkbenchTab[],
  source: ResourceRef,
): WorkbenchTab[] {
  return tabs.filter(
    (tab) =>
      (tab.kind === "file" || tab.kind === "diff") &&
      sameResource(tab.resource, source),
  );
}

export function fileHasUnsavedChanges(
  state: { tabs: readonly WorkbenchTab[] },
  source: ResourceRef,
): boolean {
  return tabsShowing(state.tabs, source).some((tab) => tab.dirty);
}
