import { Ellipsis, FolderPlus } from "lucide-react";
import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";

import { useWorkbenchStore } from "../../app/store";
import { selectOpenProject } from "../../app/workbenchSelectors";
import { explorerSelectionAction } from "./explorerActionDialogController";
import { RenameFileControl } from "./RenameFileControl";
import { RenameFolderControl } from "./RenameFolderControl";
import { TrashResourceControl } from "./TrashResourceControl";

function rovingMenuItems(container: HTMLElement) {
  return Array.from(
    container.querySelectorAll<HTMLElement>('[role="menuitem"]'),
  );
}

function activateMenuItem(items: HTMLElement[], next: number) {
  items.forEach((item, index) => {
    item.tabIndex = index === next ? 0 : -1;
  });
  items[next]?.focus();
}

export function ExplorerActionMenu() {
  const project = useWorkbenchStore(selectOpenProject);
  const projectGeneration = useWorkbenchStore(
    (state) => state.projectGeneration,
  );
  const transitionEpoch = useWorkbenchStore(
    (state) => state.nativeTransitionEpoch,
  );
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const selectedResource = useWorkbenchStore((state) => state.selectedResource);
  const directoryListings = useWorkbenchStore(
    (state) => state.directoryListings,
  );
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activeEditor = useWorkbenchStore((state) => state.activeEditor);
  const action = explorerSelectionAction({
    activeEditor,
    directoryListings,
    selectedResource,
    tabs,
  });
  const addProjectRoot = useWorkbenchStore((state) => state.addProjectRoot);
  const [open, setOpen] = useState(false);
  const wrapperRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const openingFocusRef = useRef<"first" | "last">("first");
  const menuId = useId();
  const actionSource = action.kind === "unavailable" ? null : action.source;
  const rootName =
    project?.roots.find((root) => root.id === actionSource?.rootId)?.name ??
    null;

  const closeMenu = useCallback((restoreFocus = true) => {
    setOpen(false);
    if (restoreFocus) {
      requestAnimationFrame(() => triggerRef.current?.focus());
    }
  }, []);

  useLayoutEffect(() => {
    if (!open) return;
    const menu = menuRef.current;
    if (!menu) return;
    const items = rovingMenuItems(menu);
    activateMenuItem(
      items,
      openingFocusRef.current === "last" ? items.length - 1 : 0,
    );
  }, [open]);

  useEffect(() => {
    if (!open || !project) return;
    const scope = {
      projectId: project.id,
      projectGeneration,
      transitionEpoch,
    };
    return useWorkbenchStore.subscribe((state) => {
      if (
        state.projectTransitioning ||
        state.project?.id !== scope.projectId ||
        state.projectGeneration !== scope.projectGeneration ||
        state.nativeTransitionEpoch !== scope.transitionEpoch
      ) {
        setOpen(false);
      }
    });
  }, [open, project, projectGeneration, transitionEpoch]);

  useEffect(() => {
    if (!open) return;
    const closeOnOutsidePointer = (event: PointerEvent) => {
      if (!wrapperRef.current?.contains(event.target as Node)) closeMenu();
    };
    document.addEventListener("pointerdown", closeOnOutsidePointer);
    return () =>
      document.removeEventListener("pointerdown", closeOnOutsidePointer);
  }, [closeMenu, open]);

  const moveFocus = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      closeMenu();
      return;
    }
    if (event.key === "Tab") {
      setOpen(false);
      return;
    }
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    const items = rovingMenuItems(event.currentTarget);
    if (items.length === 0) return;
    event.preventDefault();
    const current = Math.max(
      0,
      items.indexOf(document.activeElement as HTMLElement),
    );
    const next =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? items.length - 1
          : event.key === "ArrowDown"
            ? (current + 1) % items.length
            : (current - 1 + items.length) % items.length;
    activateMenuItem(items, next);
  };

  return (
    <div ref={wrapperRef} className="explorer-action-menu">
      <button
        ref={triggerRef}
        type="button"
        aria-label="More Explorer actions"
        aria-controls={menuId}
        aria-expanded={open}
        aria-haspopup="menu"
        title={project ? "More Explorer actions" : "Open a Project first"}
        disabled={!project || projectTransitioning}
        onClick={() => {
          openingFocusRef.current = "first";
          setOpen((current) => !current);
        }}
        onKeyDown={(event) => {
          if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
          event.preventDefault();
          openingFocusRef.current = event.key === "ArrowUp" ? "last" : "first";
          setOpen(true);
        }}
      >
        <Ellipsis size={15} aria-hidden="true" />
      </button>
      <div
        ref={menuRef}
        id={menuId}
        className="explorer-action-menu__surface"
        role="menu"
        aria-label="Explorer actions"
        hidden={!open}
        onKeyDown={moveFocus}
        onFocus={(event) => {
          const item = (event.target as HTMLElement).closest<HTMLElement>(
            '[role="menuitem"]',
          );
          if (!item) return;
          const items = rovingMenuItems(event.currentTarget);
          const index = items.indexOf(item);
          if (index >= 0) activateMenuItem(items, index);
        }}
      >
        <button
          className="explorer-action-menu__item"
          type="button"
          role="menuitem"
          tabIndex={0}
          onClick={() => {
            closeMenu();
            void addProjectRoot();
          }}
        >
          <FolderPlus size={14} aria-hidden="true" />
          <span>Add folder root</span>
        </button>
        {action.kind === "file" ? (
          <>
            <RenameFileControl
              source={action.source}
              rootName={rootName}
              returnFocusRef={triggerRef}
              onDialogOpen={() => setOpen(false)}
            />
            <TrashResourceControl
              source={action.source}
              kind="file"
              rootName={rootName}
              returnFocusRef={triggerRef}
              onDialogOpen={() => setOpen(false)}
            />
          </>
        ) : action.kind === "emptyFolder" ? (
          <>
            <RenameFolderControl
              source={action.source}
              rootName={rootName}
              returnFocusRef={triggerRef}
              onDialogOpen={() => setOpen(false)}
            />
            <TrashResourceControl
              source={action.source}
              kind="emptyFolder"
              rootName={rootName}
              returnFocusRef={triggerRef}
              onDialogOpen={() => setOpen(false)}
            />
          </>
        ) : (
          <div
            className="explorer-action-menu__reason"
            role="menuitem"
            aria-disabled="true"
            tabIndex={-1}
          >
            {action.reason}
          </div>
        )}
      </div>
    </div>
  );
}
