import { create } from "zustand";

import type { ResourceRef } from "../../domain/workbench";

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

export const openQuickOpen = () => {
  if (!useQuickOpenStore.getState().open) openWith("");
};
export const openCommandPalette = () => openWith(">");
export const closeQuickOpen = () => useQuickOpenStore.setState({ open: false });

const MAX_RECENT_FILES = 50;
let recentProject: string | null = null;
let recent: ResourceRef[] = [];

export function recentFiles(projectId: string): readonly ResourceRef[] {
  return projectId === recentProject ? recent : [];
}

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
