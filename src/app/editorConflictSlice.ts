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
  /** The File the banner's Compare is showing, as its disk version. */
  editorCompareKey: string | null;
  setEditorCompareKey: (key: string | null) => void;
  /**
   * "Keep mine": puts the active buffer on top of the File as the banner
   * showed it on disk, so the next save writes it; for a deleted File, the
   * next save creates it again.
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
  editorCompareKey: null,
  setEditorCompareKey: (editorCompareKey) => set({ editorCompareKey }),
  dismissSaveAllReport: () => set({ saveAllReport: null }),
  keepActiveEditorBuffer: async () =>
    (await loadActions()).keepActiveEditorBuffer(set, get),
  saveAllEditors: async () => (await loadActions()).saveAllEditors(set, get),
  closeEditorWithoutSaving: async (tabId) => {
    if (get().activeTabId !== tabId) return;
    if (await get().discardActiveEditor()) get().closeTab(tabId);
  },
});
