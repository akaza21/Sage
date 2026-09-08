import type { StateCreator } from "zustand";

import type { ResourceRef } from "../domain/workbench";
import type { FileNavigationSelectionIntent } from "../features/files/fileNavigation";
import { matchesActiveTabIntent, matchesResourceIntent } from "./projectIntent";
import type { WorkbenchState } from "./store";

export interface FileLocationNavigationRequest {
  projectId: string;
  transitionEpoch: number;
  resource: ResourceRef;
  lineNumber: number;
  columnNumber: number;
  selection: FileNavigationSelectionIntent;
}

export interface FileNavigationTarget extends FileLocationNavigationRequest {
  projectGeneration: number;
}

export interface FileNavigationSlice {
  fileNavigationTarget: FileNavigationTarget | null;
  consumeFileNavigationTarget: (target: FileNavigationTarget) => void;
  openFileLocation: (request: FileLocationNavigationRequest) => Promise<void>;
}

const MAX_NAVIGATION_IDENTIFIER_BYTES = 512;
const MAX_NAVIGATION_PATH_BYTES = 16 * 1024;
const MAX_NAVIGATION_QUERY_BYTES = 512;
const MAX_NAVIGATION_POSITION = 0xffff_ffff;

function boundedNavigationString(value: unknown, maxBytes: number) {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    value.length <= maxBytes &&
    !value.includes("\0") &&
    utf8ByteLength(value) <= maxBytes
  );
}

function validFileNavigation(request: FileLocationNavigationRequest) {
  const path = request.resource.relativePath;
  if (
    !boundedNavigationString(
      request.projectId,
      MAX_NAVIGATION_IDENTIFIER_BYTES,
    ) ||
    !boundedNavigationString(
      request.resource.rootId,
      MAX_NAVIGATION_IDENTIFIER_BYTES,
    ) ||
    !boundedNavigationString(path, MAX_NAVIGATION_PATH_BYTES)
  ) {
    return false;
  }
  const segments = path.split("/");
  return (
    !path.includes("\\") &&
    !path.startsWith("/") &&
    !path.endsWith("/") &&
    segments.every(
      (segment) => segment !== "" && segment !== "." && segment !== "..",
    ) &&
    Number.isSafeInteger(request.transitionEpoch) &&
    request.transitionEpoch >= 0 &&
    Number.isSafeInteger(request.lineNumber) &&
    request.lineNumber >= 1 &&
    request.lineNumber <= MAX_NAVIGATION_POSITION &&
    Number.isSafeInteger(request.columnNumber) &&
    request.columnNumber >= 1 &&
    request.columnNumber <= MAX_NAVIGATION_POSITION &&
    (request.selection.kind === "caret" ||
      (request.selection.kind === "match" &&
        boundedNavigationString(
          request.selection.query,
          MAX_NAVIGATION_QUERY_BYTES,
        ) &&
        typeof request.selection.caseSensitive === "boolean"))
  );
}

export const createFileNavigationSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  FileNavigationSlice
> = (set, get) => ({
  fileNavigationTarget: null,
  consumeFileNavigationTarget: (target) =>
    set((state) =>
      state.fileNavigationTarget === target
        ? { fileNavigationTarget: null }
        : {},
    ),

  openFileLocation: async (request) => {
    const generation = get().projectGeneration;
    if (
      get().projectTransitioning ||
      get().nativeTransitionEpoch !== request.transitionEpoch ||
      !validFileNavigation(request) ||
      !matchesResourceIntent(
        get(),
        generation,
        request.projectId,
        request.resource,
      )
    ) {
      return;
    }
    set({ fileNavigationTarget: null });
    await get().openFile(request.resource);
    if (
      get().projectTransitioning ||
      get().nativeTransitionEpoch !== request.transitionEpoch ||
      !matchesActiveTabIntent(
        get(),
        generation,
        request.projectId,
        "file",
        request.resource,
      )
    ) {
      return;
    }
    set({
      fileNavigationTarget: { ...request, projectGeneration: generation },
    });
  },
});
import { utf8ByteLength } from "../domain/text";
