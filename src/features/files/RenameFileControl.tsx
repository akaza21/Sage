import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
} from "react";
import { FilePenLine } from "lucide-react";

import { useWorkbenchStore } from "../../app/store";
import { parentResource, sameResource } from "../../domain/resourceIdentity";
import type { ResourceRef } from "../../domain/workbench";
import { renameTargetResource } from "./fileRenameContract";

export function RenameFileControl({
  source,
  rootName,
}: {
  source: ResourceRef | null;
  rootName: string | null;
}) {
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const inFlight = useWorkbenchStore((state) => state.fileRenameInFlight);
  const nativeError = useWorkbenchStore((state) => state.fileRenameError);
  const renameProjectFile = useWorkbenchStore(
    (state) => state.renameProjectFile,
  );
  const clearError = useWorkbenchStore((state) => state.clearFileRenameError);
  const sourceOpen = useWorkbenchStore((state) =>
    state.tabs.some(
      (tab) =>
        (tab.kind === "file" || tab.kind === "diff") &&
        sameResource(tab.resource, source),
    ),
  );
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
    if (!source || inFlight) return;
    try {
      renameTargetResource(source, basename);
    } catch (error) {
      setLocalError(error instanceof Error ? error.message : String(error));
      return;
    }
    setLocalError(null);
    if (await renameProjectFile(source, basename)) close();
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

  const parent = source ? parentResource(source) : null;
  const previewParent = parent?.relativePath
    ? `${rootName ?? "Root"}/${parent.relativePath}`
    : (rootName ?? "Root");
  const triggerTitle = sourceOpen
    ? "Close the source File and Diff tabs before renaming"
    : source
      ? "Rename File"
      : "Select a visible regular file";

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        aria-label="Rename File"
        title={triggerTitle}
        disabled={!source || sourceOpen || projectTransitioning}
        onClick={() => {
          const state = useWorkbenchStore.getState();
          if (!state.project || !source) return;
          clearError();
          setScope({
            projectId: state.project.id,
            projectGeneration: state.projectGeneration,
            transitionEpoch: state.nativeTransitionEpoch,
          });
          setBasename(source.relativePath.split("/").at(-1) ?? "");
          setLocalError(null);
          setOpen(true);
        }}
      >
        <FilePenLine size={14} aria-hidden="true" />
      </button>
      {open && source && scope ? (
        <dialog
          ref={dialogRef}
          className="dirty-dialog new-file-dialog"
          aria-modal="true"
          aria-labelledby="rename-file-title"
          onCancel={(event) => {
            event.preventDefault();
            close();
          }}
          onKeyDown={containFocus}
        >
          <form className="new-file-dialog__form" onSubmit={submit}>
            <h2 id="rename-file-title">Rename File</h2>
            <p>Rename one closed file in its current Explorer directory.</p>
            <label className="new-file-dialog__field">
              <span>File name</span>
              <input
                autoFocus
                aria-describedby="rename-file-preview rename-file-error"
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
            <p id="rename-file-preview" className="new-file-dialog__preview">
              Path: {previewParent}/{basename}
            </p>
            <p
              id="rename-file-error"
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
                {inFlight ? "Renaming…" : "Rename"}
              </button>
            </div>
          </form>
        </dialog>
      ) : null}
    </>
  );
}
