import { type StateCreator } from "zustand";

import type { ResourceRef } from "../domain/workbench";
import type { WorkbenchState } from "./store";

export interface SaveAllFailure {
  resource: ResourceRef;
  message: string;
}

export interface SaveAllReport {
  projectId: string;
  failures: SaveAllFailure[];
}

/**
 * The actions live in `editorConflictActions`, which loads on first use,
 * outside the initial bundle.
 */
export interface EditorConflictSlice {
  saveAllReport: SaveAllReport | null;
  editorCompareKey: string | null;
}

export const createEditorConflictSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  EditorConflictSlice
> = () => ({
  saveAllReport: null,
  editorCompareKey: null,
});
