import {
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type RefObject,
} from "react";

import {
  fileHasUnsavedChanges,
  UNSAVED_FILE_REASON,
} from "../../app/unsavedFile";
import { useWorkbenchStore, type WorkbenchState } from "../../app/store";
import {
  parentResource,
  resourceKey,
  sameOrDescendantResource,
  sameResource,
} from "../../domain/resourceIdentity";
import type { ResourceRef, TrashResourceKind } from "../../domain/workbench";
import { ExplorerReleasesOpenFiles } from "./explorerOpenFiles";

export type ExplorerSelectionAction =
  | { kind: "file"; source: ResourceRef }
  | { kind: "emptyFolder"; source: ResourceRef }
  | { kind: "unavailable"; reason: string };

type ExplorerSelectionState = Pick<
  WorkbenchState,
  "directoryListings" | "selectedResource" | "tabs"
> & { activeEditor: { resource: ResourceRef } | null };

export function explorerSelectionAction(
  state: ExplorerSelectionState,
  releaseOpenFiles = false,
): ExplorerSelectionAction {
  const source = state.selectedResource;
  if (!source || source.relativePath.length === 0) {
    return {
      kind: "unavailable",
      reason: "Select a visible closed file or empty folder.",
    };
  }

  const parentListing =
    state.directoryListings[resourceKey(parentResource(source))];
  if (parentListing?.phase !== "ready" || parentListing.value.truncated) {
    return {
      kind: "unavailable",
      reason: "Explorer cannot act on a selection from an incomplete listing.",
    };
  }

  const entry = parentListing.value.entries.find((candidate) =>
    sameResource(candidate.resource, source),
  );
  if (!entry || (entry.kind !== "file" && entry.kind !== "directory")) {
    return {
      kind: "unavailable",
      reason: "Select a visible closed file or empty folder.",
    };
  }

  if (entry.kind === "file" && releaseOpenFiles) {
    // An open File is closed for the move and reopened after it.
    return fileHasUnsavedChanges(state, source)
      ? { kind: "unavailable", reason: UNSAVED_FILE_REASON }
      : { kind: "file", source };
  }

  const sourceOpen =
    state.tabs.some(
      (tab) =>
        (tab.kind === "file" || tab.kind === "diff") &&
        sameOrDescendantResource(tab.resource, source),
    ) || sameOrDescendantResource(state.activeEditor?.resource, source);
  if (sourceOpen) {
    return {
      kind: "unavailable",
      reason: "Close this resource before renaming or recycling it.",
    };
  }
  if (entry.kind === "file") return { kind: "file", source };

  const folderListing = state.directoryListings[resourceKey(source)];
  if (folderListing?.phase !== "ready" || folderListing.value.truncated) {
    return {
      kind: "unavailable",
      reason: "Explorer cannot act on a selection from an incomplete listing.",
    };
  }
  if (folderListing.value.entries.length > 0) {
    return {
      kind: "unavailable",
      reason: "Only empty folders can be renamed or recycled.",
    };
  }
  return { kind: "emptyFolder", source };
}

export interface ExplorerActionTarget {
  kind: TrashResourceKind;
  source: ResourceRef;
}

interface CapturedExplorerActionTarget extends ExplorerActionTarget {
  projectId: string;
  projectGeneration: number;
  transitionEpoch: number;
}

export interface ExplorerActionDialogController {
  target: CapturedExplorerActionTarget | null;
  openDialog: (target: ExplorerActionTarget) => boolean;
  closeDialog: (restoreFocus?: boolean, force?: boolean) => void;
  runWithTarget: (
    mutation: (target: ExplorerActionTarget) => Promise<boolean>,
  ) => Promise<boolean | null>;
}

function targetIsCurrent(
  state: WorkbenchState,
  target: CapturedExplorerActionTarget,
  releaseOpenFiles: boolean,
) {
  if (
    state.projectTransitioning ||
    state.project?.id !== target.projectId ||
    state.projectGeneration !== target.projectGeneration ||
    state.nativeTransitionEpoch !== target.transitionEpoch
  ) {
    return false;
  }
  const action = explorerSelectionAction(state, releaseOpenFiles);
  return (
    action.kind === target.kind && sameResource(action.source, target.source)
  );
}

export function useExplorerActionDialog({
  clearError,
  inFlight,
  returnFocusRef,
}: {
  clearError: () => void;
  inFlight: boolean;
  returnFocusRef: RefObject<HTMLButtonElement | null>;
}): ExplorerActionDialogController {
  const [target, setTarget] = useState<CapturedExplorerActionTarget | null>(
    null,
  );
  const targetRef = useRef<CapturedExplorerActionTarget | null>(null);
  const closingRef = useRef(false);
  const ownMutationRef = useRef(false);
  const releaseOpenFiles = useContext(ExplorerReleasesOpenFiles);

  const closeDialog = useCallback(
    (restoreFocus = true, force = false) => {
      if ((inFlight && !force) || closingRef.current) return;
      closingRef.current = true;
      targetRef.current = null;
      setTarget(null);
      clearError();
      if (restoreFocus) queueMicrotask(() => returnFocusRef.current?.focus());
    },
    [clearError, inFlight, returnFocusRef],
  );

  const openDialog = useCallback(
    (nextTarget: ExplorerActionTarget) => {
      const state = useWorkbenchStore.getState();
      if (!state.project) return false;
      const captured: CapturedExplorerActionTarget = {
        kind: nextTarget.kind,
        source: { ...nextTarget.source },
        projectId: state.project.id,
        projectGeneration: state.projectGeneration,
        transitionEpoch: state.nativeTransitionEpoch,
      };
      if (!targetIsCurrent(state, captured, releaseOpenFiles)) return false;
      closingRef.current = false;
      clearError();
      targetRef.current = captured;
      setTarget(captured);
      return true;
    },
    [clearError, releaseOpenFiles],
  );

  useEffect(() => {
    if (!target) return;
    return useWorkbenchStore.subscribe((state) => {
      if (
        !ownMutationRef.current &&
        !targetIsCurrent(state, target, releaseOpenFiles)
      ) {
        closeDialog(false, true);
      }
    });
  }, [closeDialog, releaseOpenFiles, target]);

  const runWithTarget = useCallback(
    async (mutation: (value: ExplorerActionTarget) => Promise<boolean>) => {
      const captured = targetRef.current;
      if (!captured || inFlight) return null;
      if (
        !targetIsCurrent(
          useWorkbenchStore.getState(),
          captured,
          releaseOpenFiles,
        )
      ) {
        closeDialog(false, true);
        return null;
      }

      ownMutationRef.current = true;
      let succeeded: boolean;
      try {
        succeeded = await mutation({
          kind: captured.kind,
          source: { ...captured.source },
        });
      } finally {
        ownMutationRef.current = false;
      }

      if (succeeded) closeDialog(true, true);
      else if (
        !targetIsCurrent(
          useWorkbenchStore.getState(),
          captured,
          releaseOpenFiles,
        )
      ) {
        closeDialog(false, true);
      }
      return succeeded;
    },
    [closeDialog, inFlight, releaseOpenFiles],
  );

  return { target, openDialog, closeDialog, runWithTarget };
}
