import { Trash2 } from "lucide-react";
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
} from "react";

import { useWorkbenchStore } from "../../app/store";
import {
  resourceKey,
  sameOrDescendantResource,
  sameResource,
} from "../../domain/resourceIdentity";
import type { ResourceRef, TrashResourceKind } from "../../domain/workbench";

interface TrashResourceControlProps {
  source: ResourceRef | null;
  kind: TrashResourceKind | null;
  rootName: string | null;
}

export function TrashResourceControl({
  source,
  kind,
  rootName,
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
  const [open, setOpen] = useState(false);
  const [scope, setScope] = useState<{
    projectId: string;
    projectGeneration: number;
    transitionEpoch: number;
  } | null>(null);
  const dialogRef = useRef<HTMLDialogElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const closingRef = useRef(false);
  const ownTrashRef = useRef(false);

  const restoreExplorerFocus = useCallback(() => {
    const selected = useWorkbenchStore.getState().selectedResource;
    const trigger = triggerRef.current;
    if (sameResource(selected, source) && trigger && !trigger.disabled) {
      trigger.focus();
      return;
    }
    if (!selected) return;
    const tree = document.querySelector<HTMLElement>(
      '[role="tree"][aria-label="Project files"]',
    );
    const candidates = tree?.querySelectorAll<HTMLElement>(
      "[data-resource-key]",
    );
    const selectedKey = resourceKey(selected);
    const rootKey = resourceKey({ rootId: selected.rootId, relativePath: "" });
    const target = Array.from(candidates ?? []).find(
      (candidate) => candidate.dataset.resourceKey === selectedKey,
    );
    const fallback = Array.from(candidates ?? []).find(
      (candidate) => candidate.dataset.resourceKey === rootKey,
    );
    (target ?? fallback)?.focus();
  }, [source]);

  const close = useCallback(
    (shouldRestoreFocus = true, force = false) => {
      if ((inFlight && !force) || closingRef.current) return;
      closingRef.current = true;
      setOpen(false);
      setScope(null);
      clearError();
      if (shouldRestoreFocus) queueMicrotask(restoreExplorerFocus);
    },
    [clearError, inFlight, restoreExplorerFocus],
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
      if (ownTrashRef.current) return;
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
    if (!source || !kind || !scope || inFlight) return;
    ownTrashRef.current = true;
    let trashed: boolean;
    try {
      trashed = await trashProjectResource(source, kind);
    } finally {
      ownTrashRef.current = false;
    }
    if (trashed) {
      close();
      return;
    }
    const state = useWorkbenchStore.getState();
    if (
      state.projectTransitioning ||
      state.project?.id !== scope.projectId ||
      state.projectGeneration !== scope.projectGeneration ||
      state.nativeTransitionEpoch !== scope.transitionEpoch
    ) {
      close(false, true);
    }
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
        "button:not([disabled])",
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

  const resourceLabel = kind === "emptyFolder" ? "empty folder" : "file";
  const path = source
    ? `${rootName ?? "Root"}/${source.relativePath}`
    : (rootName ?? "Root");
  const triggerTitle = sourceOpen
    ? "Close File and Diff tabs for this resource and its descendants"
    : source && kind
      ? "Move to Recycle Bin"
      : "Select one visible closed file or empty folder";

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        aria-label="Move to Recycle Bin"
        title={triggerTitle}
        disabled={!source || !kind || sourceOpen || projectTransitioning}
        onClick={() => {
          const state = useWorkbenchStore.getState();
          if (!state.project || !source || !kind) return;
          closingRef.current = false;
          clearError();
          setScope({
            projectId: state.project.id,
            projectGeneration: state.projectGeneration,
            transitionEpoch: state.nativeTransitionEpoch,
          });
          setOpen(true);
        }}
      >
        <Trash2 size={14} aria-hidden="true" />
      </button>
      {open && source && kind && scope ? (
        <dialog
          ref={dialogRef}
          className="dirty-dialog new-file-dialog"
          aria-modal="true"
          aria-labelledby="trash-resource-title"
          onCancel={(event) => {
            event.preventDefault();
            close();
          }}
          onKeyDown={containFocus}
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
                onClick={() => close()}
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
        </dialog>
      ) : null}
    </>
  );
}
