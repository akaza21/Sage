import { type StateCreator } from "zustand";

import type { ResourceRef } from "../domain/workbench";
import type { WorkbenchState } from "./store";

export interface SaveAllFailure {
  resource: ResourceRef;
  message: string;
}

/** What the last Save All could not save, for the project it ran in. */
export interface SaveAllReport {
  projectId: string;
  failures: SaveAllFailure[];
}

export interface EditorConflictSlice {
  saveAllReport: SaveAllReport | null;
  /**
   * "Keep mine": keeps the active buffer over the File's change on disk, so
   * the next save writes it; for a deleted File, the next save recreates it.
   */
  keepActiveEditorBuffer: () => Promise<boolean>;
  /** Saves every dirty File tab, carrying on past the ones that fail. */
  saveAllEditors: () => Promise<SaveAllFailure[]>;
  dismissSaveAllReport: () => void;
  /** Drops the active File's unsaved changes and closes its tab. */
  closeEditorWithoutSaving: (tabId: string) => Promise<void>;
}

const loadActions = () => import("./editorConflictActions");

export const createEditorConflictSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  EditorConflictSlice
> = (set, get) => ({
  saveAllReport: null,
  dismissSaveAllReport: () => set({ saveAllReport: null }),
  keepActiveEditorBuffer: async () =>
    (await loadActions()).keepActiveEditorBuffer(set, get),
  saveAllEditors: async () => (await loadActions()).saveAllEditors(set, get),
  closeEditorWithoutSaving: async (tabId) => {
    if (get().activeTabId !== tabId) return;
    if (await get().discardActiveEditor()) get().closeTab(tabId);
  },
});
