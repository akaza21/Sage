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

/**
 * State for the conflict banner and Save All. Their actions are in
 * `editorConflictActions`, which loads on first use, outside the initial
 * bundle.
 */
export interface EditorConflictSlice {
  saveAllReport: SaveAllReport | null;
  /** The File the banner's Compare is showing, as its disk version. */
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
