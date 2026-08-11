import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
} from "react";
import { FilePlus2 } from "lucide-react";

import { useWorkbenchStore } from "../../app/store";
import type { ResourceRef } from "../../domain/workbench";
import { validateNewFilePath } from "./newFileContract";

export function NewFileControl({
  parent,
  rootName,
}: {
  parent: ResourceRef | null;
  rootName: string | null;
}) {
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const inFlight = useWorkbenchStore((state) => state.fileCreateInFlight);
  const nativeError = useWorkbenchStore((state) => state.fileCreateError);
  const createProjectFile = useWorkbenchStore(
    (state) => state.createProjectFile,
  );
  const clearError = useWorkbenchStore((state) => state.clearFileCreateError);
  const [open, setOpen] = useState(false);
  const [basename, setBasename] = useState("");
  const [localError, setLocalError] = useState<string | null>(null);
  const [scope, setScope] = useState<{
    projectId: string;
    projectGeneration: number;
    transitionEpoch: number;
  } | null>(null);
  const dialogRef = useRef<HTMLDialogElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);

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
    const dialog = dialogRef.current;
    if (!open || !dialog) return;
    if (typeof dialog.showModal === "function") {
      if (!dialog.open) dialog.showModal();
    } else {
      dialog.setAttribute("open", "");
    }
    return () => {
      if (typeof dialog.close === "function" && dialog.open) dialog.close();
      else dialog.removeAttribute("open");
    };
  }, [open]);

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
    const error = validateNewFilePath(parent, basename);
    if (error) {
      setLocalError(error);
      return;
    }
    setLocalError(null);
    if (await createProjectFile(parent, basename)) close();
  };

  const containFocus = (event: KeyboardEvent<HTMLDialogElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
      return;
    }
    if (event.key !== "Tab") return;
    const focusable = Array.from(
      event.currentTarget.querySelectorAll<HTMLElement>(
        "input:not([disabled]), button:not([disabled])",
      ),
    );
    const first = focusable[0];
    const last = focusable.at(-1);
    if (!first || !last) event.preventDefault();
    else if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
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
        aria-label="New File"
        title={parent ? "New File" : "Select an available Project root"}
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
        <FilePlus2 size={14} aria-hidden="true" />
      </button>
      {open && parent && scope ? (
        <dialog
          ref={dialogRef}
          className="dirty-dialog new-file-dialog"
          aria-modal="true"
          aria-labelledby="new-file-title"
          onCancel={(event) => {
            event.preventDefault();
            close();
          }}
          onKeyDown={containFocus}
        >
          <form className="new-file-dialog__form" onSubmit={submit}>
            <h2 id="new-file-title">New File</h2>
            <p>Create one empty file in the selected Explorer directory.</p>
            <label className="new-file-dialog__field">
              <span>File name</span>
              <input
                autoFocus
                aria-describedby="new-file-preview new-file-error"
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
            <p id="new-file-preview" className="new-file-dialog__preview">
              Path: {preview}
            </p>
            <p
              id="new-file-error"
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
                onClick={() => close()}
              >
                Cancel
              </button>
              <button
                className="button button--primary"
                type="submit"
                disabled={inFlight || basename.length === 0}
              >
                {inFlight ? "Creating…" : "Create"}
              </button>
            </div>
          </form>
        </dialog>
      ) : null}
    </>
  );
}
