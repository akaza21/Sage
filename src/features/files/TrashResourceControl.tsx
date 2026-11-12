import { Trash2 } from "../../ui/leanIcons";
import { useContext, type FormEvent, type RefObject } from "react";

import { useWorkbenchStore } from "../../app/store";
import { sameOrDescendantResource } from "../../domain/resourceIdentity";
import type { ResourceRef, TrashResourceKind } from "../../domain/workbench";
import { ModalDialog } from "../../ui/ModalDialog";
import { trashName } from "../../ui/trashName";
import { useExplorerActionDialog } from "./explorerActionDialogController";
import { ExplorerReleasesOpenFiles } from "./explorerOpenFiles";

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
  // In the Editor an open File is closed for the move.
  const releasesOpenFile =
    useContext(ExplorerReleasesOpenFiles) && kind === "file";
  const sourceOpen = useWorkbenchStore((state) =>
    source && !releasesOpenFile
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
      releasesOpenFile && target.kind === "file"
        ? trashProjectResource(target.source, target.kind, true)
        : trashProjectResource(target.source, target.kind),
    );
  };

  const moveLabel = `Move to ${trashName()}`;
  const displayTarget =
    dialog.target ?? (source && kind ? { source, kind } : null);
  const resourceLabel =
    displayTarget?.kind === "emptyFolder" ? "empty folder" : "file";
  const path = displayTarget
    ? `${rootName ?? "Root"}/${displayTarget.source.relativePath}`
    : (rootName ?? "Root");
  const triggerTitle = sourceOpen
    ? "Close file and diff tabs for this resource and its descendants"
    : source && kind
      ? moveLabel
      : "Select one visible closed file or empty folder";

  return (
    <>
      <button
        className="menu-item explorer-action-menu__item"
        type="button"
        role="menuitem"
        tabIndex={-1}
        aria-label={moveLabel}
        title={triggerTitle}
        disabled={!source || !kind || sourceOpen || projectTransitioning}
        onClick={() => {
          if (!source || !kind || !dialog.openDialog({ kind, source })) return;
          onDialogOpen();
        }}
      >
        <Trash2 size={14} aria-hidden="true" />
        <span>{moveLabel}</span>
      </button>
      {dialog.target ? (
        <ModalDialog
          className="new-file-dialog new-file-dialog--editor"
          labelledBy="trash-resource-title"
          onDismiss={() => dialog.closeDialog()}
        >
          <form className="new-file-dialog__form" onSubmit={submit}>
            <h2 id="trash-resource-title">{moveLabel}?</h2>
            <p>
              Move this {resourceLabel} to the {trashName()}. You can restore it
              from there.
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
                {inFlight ? "Moving…" : moveLabel}
              </button>
            </div>
          </form>
        </ModalDialog>
      ) : null}
    </>
  );
}
