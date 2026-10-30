import { create } from "zustand";

import type { ResourceRef } from "../../domain/workbench";

/** Whether Quick Open is showing. Its UI loads on first open. */
export const useQuickOpenStore = create<{ open: boolean }>(() => ({
  open: false,
}));

export const openQuickOpen = () => useQuickOpenStore.setState({ open: true });
export const closeQuickOpen = () => useQuickOpenStore.setState({ open: false });

/** Quick Open sends at most this many recently opened Files. */
const MAX_RECENT_FILES = 50;
let recentProject: string | null = null;
let recent: ResourceRef[] = [];

/** Recently opened Files of `projectId`, most recent first. */
export function recentFiles(projectId: string): readonly ResourceRef[] {
  return projectId === recentProject ? recent : [];
}

/** Notes that `resource` of `projectId` was just shown. */
export function noteRecentFile(projectId: string, resource: ResourceRef) {
  if (projectId !== recentProject) {
    recentProject = projectId;
    recent = [];
  }
  recent = [
    resource,
    ...recent.filter(
      (item) =>
        item.rootId !== resource.rootId ||
        item.relativePath !== resource.relativePath,
    ),
  ].slice(0, MAX_RECENT_FILES);
}
