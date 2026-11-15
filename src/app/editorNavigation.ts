import { isCompactWorkbenchViewport } from "../workbench/workbenchViewport";

export function editorNavigationUpdate() {
  return isCompactWorkbenchViewport() ? { sidebarOpen: false } : undefined;
}
