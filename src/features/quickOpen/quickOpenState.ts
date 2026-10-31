import { create } from "zustand";

import type { ResourceRef } from "../../domain/workbench";

/**
 * Whether Quick Open is showing, and the text it starts with: ">" lists
 * commands (ED-2a). Each opening has its own number, so a new one starts
 * afresh. Its UI loads on first open.
 */
export const useQuickOpenStore = create<{
  open: boolean;
  seed: string;
  opening: number;
}>(() => ({ open: false, seed: "", opening: 0 }));

const openWith = (seed: string) =>
  useQuickOpenStore.setState((state) => ({
    open: true,
    seed,
    opening: state.opening + 1,
  }));

/** Opens Quick Open on Files; while open, it stays as it is. */
export const openQuickOpen = () => {
  if (!useQuickOpenStore.getState().open) openWith("");
};
/** Opens Quick Open on the editor's commands. */
export const openCommandPalette = () => openWith(">");
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
