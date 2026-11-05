import { isCompactWorkbenchViewport } from "../workbench/workbenchViewport";

/** Resource navigation dismisses a covering drawer, while preserving docked tools. */
export function editorNavigationUpdate() {
  return isCompactWorkbenchViewport() ? { sidebarOpen: false } : undefined;
}
