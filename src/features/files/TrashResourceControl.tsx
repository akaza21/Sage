import { Trash2 } from "lucide-react";
import { type FormEvent, type RefObject } from "react";

import { useWorkbenchStore } from "../../app/store";
import { sameOrDescendantResource } from "../../domain/resourceIdentity";
import type { ResourceRef, TrashResourceKind } from "../../domain/workbench";
import { ModalDialog } from "../../ui/ModalDialog";
import { useExplorerActionDialog } from "./explorerActionDialogController";

interface TrashResourceControlProps {
  source: ResourceRef | null;
  kind: TrashResourceKind | null;
  rootName: string | null;
  returnFocusRef: RefObject<HTMLButtonElement | null>;
  onDialogOpen: () => void;
}

export function TrashResourceControl({
  source,
  kind,
  rootName,
  returnFocusRef,
  onDialogOpen,
}: TrashResourceControlProps) {
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const inFlight = useWorkbenchStore((state) => state.resourceTrashInFlight);
  const nativeError = useWorkbenchStore((state) => state.resourceTrashError);
  const trashProjectResource = useWorkbenchStore(
    (state) => state.trashProjectResource,
  );
  const clearError = useWorkbenchStore(
    (state) => state.clearResourceTrashError,
  );
  const sourceOpen = useWorkbenchStore((state) =>
    source
      ? state.tabs.some(
          (tab) =>
            (tab.kind === "file" || tab.kind === "diff") &&
            sameOrDescendantResource(tab.resource, source),
        ) || sameOrDescendantResource(state.activeEditor?.resource, source)
      : false,
  );
  const dialog = useExplorerActionDialog({
    clearError,
    inFlight,
    returnFocusRef,
  });

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!dialog.target || inFlight) return;
    await dialog.runWithTarget((target) =>
      trashProjectResource(target.source, target.kind),
    );
  };

  const displayTarget =
    dialog.target ?? (source && kind ? { source, kind } : null);
  const resourceLabel =
    displayTarget?.kind === "emptyFolder" ? "empty folder" : "file";
  const path = displayTarget
    ? `${rootName ?? "Root"}/${displayTarget.source.relativePath}`
    : (rootName ?? "Root");
  const triggerTitle = sourceOpen
    ? "Close File and Diff tabs for this resource and its descendants"
    : source && kind
      ? "Move to Recycle Bin"
      : "Select one visible closed file or empty folder";

  return (
    <>
      <button
        className="menu-item explorer-action-menu__item"
        type="button"
        role="menuitem"
        tabIndex={-1}
        aria-label="Move to Recycle Bin"
        title={triggerTitle}
        disabled={!source || !kind || sourceOpen || projectTransitioning}
        onClick={() => {
          if (!source || !kind || !dialog.openDialog({ kind, source })) return;
          onDialogOpen();
        }}
      >
        <Trash2 size={14} aria-hidden="true" />
        <span>Move to Recycle Bin</span>
      </button>
      {dialog.target ? (
        <ModalDialog
          className="new-file-dialog new-file-dialog--editor"
          labelledBy="trash-resource-title"
          onDismiss={() => dialog.closeDialog()}
        >
          <form className="new-file-dialog__form" onSubmit={submit}>
            <h2 id="trash-resource-title">Move to Recycle Bin?</h2>
            <p>
              Move this {resourceLabel} to the operating system Recycle Bin.
            </p>
            <p className="new-file-dialog__preview">Path: {path}</p>
            <p className="new-file-dialog__error" role="alert">
              {nativeError ?? ""}
            </p>
            <div className="dirty-dialog__actions">
              <button
                className="button"
                type="button"
                disabled={inFlight}
                onClick={() => dialog.closeDialog()}
              >
                Cancel
              </button>
              <button
                autoFocus
                className="button button--primary"
                type="submit"
                disabled={inFlight}
              >
                {inFlight ? "Moving…" : "Move to Recycle Bin"}
              </button>
            </div>
          </form>
        </ModalDialog>
      ) : null}
    </>
  );
}
