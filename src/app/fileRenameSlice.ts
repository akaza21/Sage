import type { StateCreator } from "zustand";

import type { ResourceRef } from "../domain/workbench";
import type { WorkbenchState } from "./store";

export interface FileRenameSlice {
  fileRenameInFlight: boolean;
  fileRenameError: string | null;
  fileRenameNotice: string | null;
  fileRenameRequestGeneration: number;
  clearFileRenameError: () => void;
  /** With `releaseOpenFile`, a clean open source File is closed for the
   * rename and reopened at the new name. */
  renameProjectFile: (
    source: ResourceRef,
    basename: string,
    releaseOpenFile?: boolean,
  ) => Promise<boolean>;
  renameProjectFolder: (
    source: ResourceRef,
    basename: string,
  ) => Promise<boolean>;
}

// The rename itself loads with the first rename a session asks for.
const actions = () => import("./fileRenameActions");

export const createFileRenameSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  FileRenameSlice
> = (set, get) => ({
  fileRenameInFlight: false,
  fileRenameError: null,
  fileRenameNotice: null,
  fileRenameRequestGeneration: 0,

  clearFileRenameError: () => set({ fileRenameError: null }),

  // Each rename reads the store as it was asked for, before its code loads.
  renameProjectFile: async (source, basename, releaseOpenFile = false) => {
    const state = get();
    const loaded = await actions();
    return releaseOpenFile
      ? loaded.renameReleasingFile(get, set, source, basename)
      : loaded.renameProjectResource(get, set, state, "file", source, basename);
  },
  renameProjectFolder: async (source, basename) => {
    const state = get();
    const loaded = await actions();
    return loaded.renameProjectResource(
      get,
      set,
      state,
      "folder",
      source,
      basename,
    );
  },
});
