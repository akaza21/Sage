import { useState, type FormEvent, type RefObject } from "react";
import { FilePenLine } from "../../ui/leanIcons";
import { useWorkbenchStore } from "../../app/store";
import {
  parentResource,
  sameOrDescendantResource,
  sameResource,
} from "../../domain/resourceIdentity";
import type { ResourceRef } from "../../domain/workbench";
import { ModalDialog } from "../../ui/ModalDialog";
import { useExplorerActionDialog } from "./explorerActionDialogController";
import { renameTargetResource } from "./fileRenameContract";
import { renameFolderTargetResource } from "./folderRenameContract";
import { errorMessage } from "../../domain/errors";

export function RenameResourceControl({
  kind,
  source,
  rootName,
  returnFocusRef,
  onDialogOpen,
}: {
  kind: "File" | "Folder";
  source: ResourceRef | null;
  rootName: string | null;
  returnFocusRef: RefObject<HTMLButtonElement | null>;
  onDialogOpen: () => void;
}) {
  const folder = kind === "Folder";
  const id = folder ? "rename-folder" : "rename-file";
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const inFlight = useWorkbenchStore((state) => state.fileRenameInFlight);
  const nativeError = useWorkbenchStore((state) => state.fileRenameError);
  const renameProjectFile = useWorkbenchStore(
    (state) => state.renameProjectFile,
  );
  const renameProjectFolder = useWorkbenchStore(
    (state) => state.renameProjectFolder,
  );
  const clearError = useWorkbenchStore((state) => state.clearFileRenameError);
  const sourceOpen = useWorkbenchStore((state) =>
    state.tabs.some(
      (tab) =>
        (tab.kind === "file" || tab.kind === "diff") &&
        (folder
          ? source !== null && sameOrDescendantResource(tab.resource, source)
          : sameResource(tab.resource, source)),
    ),
  );
  const [basename, setBasename] = useState("");
  const [localError, setLocalError] = useState<string | null>(null);
  const dialog = useExplorerActionDialog({
    clearError,
    inFlight,
    returnFocusRef,
  });

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const capturedSource = dialog.target?.source;
    if (!capturedSource || inFlight) return;
    try {
      if (folder) renameFolderTargetResource(capturedSource, basename);
      else renameTargetResource(capturedSource, basename);
    } catch (error) {
      setLocalError(errorMessage(error));
      return;
    }
    setLocalError(null);
    await dialog.runWithTarget((target) =>
      folder
        ? renameProjectFolder(target.source, basename)
        : renameProjectFile(target.source, basename),
    );
  };

  const displaySource = dialog.target?.source ?? source;
  const parent = displaySource ? parentResource(displaySource) : null;
  const previewParent = parent?.relativePath
    ? `${rootName ?? "Root"}/${parent.relativePath}`
    : (rootName ?? "Root");
  const triggerTitle = sourceOpen
    ? folder
      ? "Close File and Diff tabs for the folder and its descendants before renaming"
      : "Close the source File and Diff tabs before renaming"
    : source
      ? `Rename ${kind}`
      : folder
        ? "Select a visible empty folder"
        : "Select a visible regular file";

  return (
    <>
      <button
        className="menu-item explorer-action-menu__item"
        type="button"
        role="menuitem"
        tabIndex={-1}
        aria-label={`Rename ${kind}`}
        title={triggerTitle}
        disabled={!source || sourceOpen || projectTransitioning}
        onClick={() => {
          if (
            !source ||
            !dialog.openDialog({
              kind: folder ? "emptyFolder" : "file",
              source,
            })
          )
            return;
          setBasename(source.relativePath.split("/").at(-1) ?? "");
          setLocalError(null);
          onDialogOpen();
        }}
      >
        <FilePenLine size={14} aria-hidden="true" />
        <span>Rename {kind}</span>
      </button>
      {dialog.target ? (
        <ModalDialog
          className="new-file-dialog new-file-dialog--editor"
          labelledBy={`${id}-title`}
          onDismiss={() => dialog.closeDialog()}
        >
          <form className="new-file-dialog__form" onSubmit={submit}>
            <h2 id={`${id}-title`}>Rename {kind}</h2>
            <p>
              Rename one {folder ? "empty folder" : "closed file"} in its
              current Explorer directory.
            </p>
            <label className="new-file-dialog__field">
              <span>{kind} name</span>
              <input
                autoFocus
                aria-describedby={`${id}-preview ${id}-error`}
                disabled={inFlight}
                maxLength={255}
                value={basename}
                onChange={(event) => {
                  setBasename(event.target.value);
                  setLocalError(null);
                  clearError();
                }}
              />
            </label>
            <p id={`${id}-preview`} className="new-file-dialog__preview">
              Path: {previewParent}/{basename}
            </p>
            <p
              id={`${id}-error`}
              className="new-file-dialog__error"
              role="alert"
            >
              {localError ?? nativeError ?? ""}
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
                className="button button--primary"
                type="submit"
                disabled={inFlight || basename.length === 0}
              >
                {inFlight ? "Renaming…" : "Rename"}
              </button>
            </div>
          </form>
        </ModalDialog>
      ) : null}
    </>
  );
}
