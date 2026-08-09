import type { StateCreator } from "zustand";

import type { WorkspaceSearchNavigationRequest } from "../features/search";
import { WORKSPACE_SEARCH_LIMITS } from "../features/search/workspaceSearchLimits";
import { matchesActiveTabIntent, matchesResourceIntent } from "./projectIntent";
import type { WorkbenchState } from "./store";

export interface FileNavigationTarget extends WorkspaceSearchNavigationRequest {
  projectGeneration: number;
}

export interface SearchNavigationSlice {
  fileNavigationTarget: FileNavigationTarget | null;
  consumeFileNavigationTarget: (target: FileNavigationTarget) => void;
  openSearchMatch: (request: WorkspaceSearchNavigationRequest) => Promise<void>;
}

const MAX_SEARCH_IDENTIFIER_BYTES = 512;
const MAX_SEARCH_POSITION = 0xffff_ffff;

function boundedSearchString(value: unknown, maxBytes: number) {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    value.length <= maxBytes &&
    !value.includes("\0") &&
    new TextEncoder().encode(value).byteLength <= maxBytes
  );
}

function validSearchNavigation(request: WorkspaceSearchNavigationRequest) {
  const path = request.resource.relativePath;
  if (
    !boundedSearchString(request.projectId, MAX_SEARCH_IDENTIFIER_BYTES) ||
    !boundedSearchString(
      request.resource.rootId,
      MAX_SEARCH_IDENTIFIER_BYTES,
    ) ||
    !boundedSearchString(path, WORKSPACE_SEARCH_LIMITS.resourcePathBytes) ||
    !boundedSearchString(request.query, WORKSPACE_SEARCH_LIMITS.queryBytes)
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
    request.lineNumber <= MAX_SEARCH_POSITION &&
    Number.isSafeInteger(request.columnNumber) &&
    request.columnNumber >= 1 &&
    request.columnNumber <= MAX_SEARCH_POSITION &&
    typeof request.caseSensitive === "boolean"
  );
}

export const createSearchNavigationSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  SearchNavigationSlice
> = (set, get) => ({
  fileNavigationTarget: null,
  consumeFileNavigationTarget: (target) =>
    set((state) =>
      state.fileNavigationTarget === target
        ? { fileNavigationTarget: null }
        : {},
    ),

  openSearchMatch: async (request) => {
    const generation = get().projectGeneration;
    if (
      get().projectTransitioning ||
      get().nativeTransitionEpoch !== request.transitionEpoch ||
      !validSearchNavigation(request) ||
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
