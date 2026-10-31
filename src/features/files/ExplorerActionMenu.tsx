import { Ellipsis, FolderPlus } from "../../ui/leanIcons";
import {
  useCallback,
  useContext,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from "react";

import { useWorkbenchStore } from "../../app/store";
import { selectOpenProject } from "../../app/workbenchSelectors";
import { MenuPopover } from "../../ui/MenuPopover";
import { explorerSelectionAction } from "./explorerActionDialogController";
import { RenameResourceControl } from "./RenameResourceControl";
import { ExplorerContextMenuContext } from "./explorerContextMenu";
import { ExplorerReleasesOpenFiles } from "./explorerOpenFiles";
import { TrashResourceControl } from "./TrashResourceControl";

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
  // The resource only: the whole editor state changes with every keystroke.
  const activeResource = useWorkbenchStore(
    (state) => state.activeEditor?.resource ?? null,
  );
  const releaseOpenFiles = useContext(ExplorerReleasesOpenFiles);
  const action = explorerSelectionAction(
    {
      activeEditor: activeResource && { resource: activeResource },
      directoryListings,
      selectedResource,
      tabs,
    },
    releaseOpenFiles,
  );
  const addProjectRoot = useWorkbenchStore((state) => state.addProjectRoot);
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const [openingFocus, setOpeningFocus] = useState<"first" | "last">("first");
  // The row a right-click opened the menu at, or null for the button.
  const [row, setRow] = useState<HTMLElement | null>(null);
  // Focus goes back to that row, or to the button if the row is gone (a
  // renamed File's row is replaced). A new object per anchor, so the popover
  // follows the row it now belongs to.
  const anchorRef = useMemo<RefObject<HTMLButtonElement | null>>(
    () => ({
      get current() {
        return row?.isConnected
          ? (row as HTMLButtonElement)
          : triggerRef.current;
      },
    }),
    [row],
  );
  // A close hands the focus back on the next frame; reopening before then
  // (a right-click on another row, the "…" button) must keep it instead.
  const focusFrame = useRef<number | null>(null);
  const keepFocus = useCallback(() => {
    if (focusFrame.current !== null) cancelAnimationFrame(focusFrame.current);
    focusFrame.current = null;
  }, []);
  const contextMenu = useContext(ExplorerContextMenuContext);
  useEffect(
    () =>
      contextMenu?.register((anchor) => {
        keepFocus();
        setRow(anchor);
        setOpeningFocus("first");
        setOpen(true);
      }),
    [contextMenu, keepFocus],
  );
  const menuId = useId();
  const actionSource = action.kind === "unavailable" ? null : action.source;
  const rootName =
    project?.roots.find((root) => root.id === actionSource?.rootId)?.name ??
    null;

  const closeMenu = useCallback(
    (restoreFocus = true) => {
      setOpen(false);
      keepFocus();
      if (restoreFocus) {
        focusFrame.current = requestAnimationFrame(() => {
          focusFrame.current = null;
          anchorRef.current?.focus();
        });
      }
    },
    [anchorRef, keepFocus],
  );

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

  return (
    <div className="explorer-action-menu">
      <button
        ref={triggerRef}
        type="button"
        aria-label="More Explorer actions"
        aria-controls={menuId}
        // A menu a row opened belongs to that row.
        aria-expanded={open && row === null}
        aria-haspopup="menu"
        title={project ? "More Explorer actions" : "Open a Project first"}
        disabled={!project || projectTransitioning}
        onClick={() => {
          keepFocus();
          setOpeningFocus("first");
          setOpen((current) => !current || row !== null);
          setRow(null);
        }}
        onKeyDown={(event) => {
          if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
          event.preventDefault();
          keepFocus();
          setOpeningFocus(event.key === "ArrowUp" ? "last" : "first");
          setRow(null);
          setOpen(true);
        }}
      >
        <Ellipsis size={16} aria-hidden="true" />
      </button>
      <MenuPopover
        id={menuId}
        open={open}
        ariaLabel="Explorer actions"
        className="menu-popover--editor explorer-action-menu__surface"
        initialFocus={openingFocus}
        keepMounted
        triggerRef={anchorRef}
        onDismiss={(reason) => {
          if (reason === "outside") closeMenu();
          else setOpen(false);
        }}
      >
        <button
          className="menu-item explorer-action-menu__item"
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
            <RenameResourceControl
              kind="File"
              source={action.source}
              rootName={rootName}
              returnFocusRef={anchorRef}
              onDialogOpen={() => setOpen(false)}
            />
            <TrashResourceControl
              source={action.source}
              kind="file"
              rootName={rootName}
              returnFocusRef={anchorRef}
              onDialogOpen={() => setOpen(false)}
            />
          </>
        ) : action.kind === "emptyFolder" ? (
          <>
            <RenameResourceControl
              kind="Folder"
              source={action.source}
              rootName={rootName}
              returnFocusRef={anchorRef}
              onDialogOpen={() => setOpen(false)}
            />
            <TrashResourceControl
              source={action.source}
              kind="emptyFolder"
              rootName={rootName}
              returnFocusRef={anchorRef}
              onDialogOpen={() => setOpen(false)}
            />
          </>
        ) : (
          <div
            className="menu-item explorer-action-menu__reason"
            role="menuitem"
            aria-disabled="true"
            tabIndex={-1}
          >
            {action.reason}
          </div>
        )}
      </MenuPopover>
    </div>
  );
}
