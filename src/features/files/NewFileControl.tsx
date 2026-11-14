import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type FormEvent,
} from "react";
import { FilePlus2, FolderPlus } from "../../ui/leanIcons";

import { useWorkbenchStore } from "../../app/store";
import { sameResource } from "../../domain/resourceIdentity";
import type { ResourceRef } from "../../domain/workbench";
import { ModalDialog } from "../../ui/ModalDialog";
import { validateNewFilePath } from "./newFileContract";

function NewEntryControl({
  parent,
  rootName,
  kind,
}: {
  parent: ResourceRef | null;
  rootName: string | null;
  kind: "file" | "folder";
}) {
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const inFlight = useWorkbenchStore((state) =>
    kind === "file" ? state.fileCreateInFlight : state.folderCreateInFlight,
  );
  const nativeError = useWorkbenchStore((state) =>
    kind === "file" ? state.fileCreateError : state.folderCreateError,
  );
  const createProjectResource = useWorkbenchStore((state) =>
    kind === "file" ? state.createProjectFile : state.createProjectFolder,
  );
  const clearError = useWorkbenchStore((state) =>
    kind === "file" ? state.clearFileCreateError : state.clearFolderCreateError,
  );
  const [open, setOpen] = useState(false);
  const [basename, setBasename] = useState("");
  const [localError, setLocalError] = useState<string | null>(null);
  const [scope, setScope] = useState<{
    projectId: string;
    projectGeneration: number;
    transitionEpoch: number;
  } | null>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const label = kind === "file" ? "File" : "Folder";
  const titleId = `new-${kind}-title`;
  const previewId = `new-${kind}-preview`;
  const errorId = `new-${kind}-error`;

  const close = useCallback(
    (restoreFocus = true, force = false) => {
      if (inFlight && !force) return;
      setOpen(false);
      setScope(null);
      setBasename("");
      setLocalError(null);
      clearError();
      if (restoreFocus) queueMicrotask(() => triggerRef.current?.focus());
    },
    [clearError, inFlight],
  );

  useEffect(() => {
    if (!open || !scope) return;
    return useWorkbenchStore.subscribe((state) => {
      if (
        state.projectTransitioning ||
        state.project?.id !== scope.projectId ||
        state.projectGeneration !== scope.projectGeneration ||
        state.nativeTransitionEpoch !== scope.transitionEpoch
      ) {
        close(false, true);
      }
    });
  }, [close, open, scope]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!parent || inFlight) return;
    const validation = validateNewFilePath(parent, basename);
    const error =
      kind === "folder" && validation
        ? validation.replace(/^File/, "Folder")
        : validation;
    if (error) {
      setLocalError(error);
      return;
    }
    setLocalError(null);
    const before = useWorkbenchStore.getState().activeEditor;
    if (!(await createProjectResource(parent, basename))) return;
    const opened = useWorkbenchStore.getState().activeEditor;
    if (
      kind === "file" &&
      opened?.phase === "ready" &&
      !(before && sameResource(before.resource, opened.resource))
    ) {
      close(false);
      void import("./editorFocusRequest").then((request) =>
        request.requestEditorFocus(opened.resource),
      );
    } else {
      close();
    }
  };

  const previewParent = parent?.relativePath
    ? `${rootName ?? "Root"}/${parent.relativePath}`
    : (rootName ?? "Root");
  const preview = basename
    ? `${previewParent}/${basename}`
    : `${previewParent}/`;

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        aria-label={`New ${label}`}
        title={parent ? `New ${label}` : "Select an available project root"}
        disabled={!parent || projectTransitioning}
        onClick={() => {
          const state = useWorkbenchStore.getState();
          if (!state.project) return;
          clearError();
          setScope({
            projectId: state.project.id,
            projectGeneration: state.projectGeneration,
            transitionEpoch: state.nativeTransitionEpoch,
          });
          setBasename("");
          setLocalError(null);
          setOpen(true);
        }}
      >
        {kind === "file" ? (
          <FilePlus2 size={14} aria-hidden="true" />
        ) : (
          <FolderPlus size={14} aria-hidden="true" />
        )}
      </button>
      {open && parent && scope ? (
        <ModalDialog
          className="new-file-dialog new-file-dialog--editor"
          labelledBy={titleId}
          onDismiss={() => close()}
        >
          <form className="new-file-dialog__form" onSubmit={submit}>
            <h2 id={titleId}>New {label}</h2>
            <p>Create one empty {kind} in the selected Explorer directory.</p>
            <label className="new-file-dialog__field">
              <span>{label} name</span>
              <input
                autoFocus
                aria-describedby={`${previewId} ${errorId}`}
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
            <p id={previewId} className="new-file-dialog__preview">
              Path: {preview}
            </p>
            <p id={errorId} className="new-file-dialog__error" role="alert">
              {localError ?? nativeError ?? ""}
            </p>
            <div className="dirty-dialog__actions">
              <button
                className="button"
                type="button"
                disabled={inFlight}
                onClick={() => close()}
              >
                Cancel
              </button>
              <button
                className="button button--primary"
                type="submit"
                disabled={inFlight || basename.length === 0}
              >
                {inFlight
                  ? "Creating…"
                  : kind === "file"
                    ? "Create"
                    : "Create folder"}
              </button>
            </div>
          </form>
        </ModalDialog>
      ) : null}
    </>
  );
}

export function NewFileControl(props: {
  parent: ResourceRef | null;
  rootName: string | null;
}) {
  return <NewEntryControl {...props} kind="file" />;
}

export function NewFolderControl(props: {
  parent: ResourceRef | null;
  rootName: string | null;
}) {
  return <NewEntryControl {...props} kind="folder" />;
}
