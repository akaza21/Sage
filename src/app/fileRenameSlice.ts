import type { StateCreator } from "zustand";

import type { ResourceRef } from "../domain/workbench";
import type { WorkbenchState } from "./store";

export interface FileRenameSlice {
  fileRenameInFlight: boolean;
  fileRenameError: string | null;
  fileRenameNotice: string | null;
  fileRenameRequestGeneration: number;
  clearFileRenameError: () => void;
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

  // The loaded rename reads the store itself, so its in-flight check and
  // claim stay in one synchronous step.
  renameProjectFile: async (source, basename, releaseOpenFile = false) => {
    const loaded = await actions();
    return releaseOpenFile
      ? loaded.renameReleasingFile(get, set, source, basename)
      : loaded.renameProjectResource(get, set, "file", source, basename);
  },
  renameProjectFolder: async (source, basename) =>
    (await actions()).renameProjectResource(
      get,
      set,
      "folder",
      source,
      basename,
    ),
});
