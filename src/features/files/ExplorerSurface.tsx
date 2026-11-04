import {
  memo,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import { useEditorSettingsStore } from "../../app/editorSettingsStore";
import { useWorkbenchStore } from "../../app/store";
import { selectOpenProject } from "../../app/workbenchSelectors";
import {
  childResource,
  resourceKey,
  sameResource,
} from "../../domain/resourceIdentity";
import type * as MatcherModule from "../../domain/pathGlobMatcher";
import type { ProjectRootSummary, ResourceRef } from "../../domain/workbench";
import type { PopoverPoint } from "../../ui/pointerPopoverPosition";
import { useRootGitDecorations } from "../git/useRootGitDecorations";
import { ExplorerActions } from "./ExplorerActions";
import {
  ExplorerContextMenuContext,
  createExplorerContextMenu,
  isContextMenuKey,
} from "./explorerContextMenu";
import { ExplorerRow } from "./ExplorerRow";
import { FileKindIcon } from "./FileKindIcon";
import {
  EXPLORER_ROW_LIMIT,
  flattenFileTree,
  type FlattenedFileTree,
} from "./fileTree";

// The `files.exclude` matcher loads beside the startup bundle, starting as
// this module loads, long before the first listing returns.
let matcherModule: typeof MatcherModule | undefined;
// A failed load is reported, and Explorer then shows excluded entries.
const matcherLoaded = import("../../domain/pathGlobMatcher").then(
  (module) => (matcherModule = module),
  (error: unknown) =>
    console.warn("Explorer could not load the files.exclude matcher", error),
);

const RootTree = memo(function RootTree({
  root,
  tree,
}: {
  root: ProjectRootSummary;
  tree: FlattenedFileTree;
}) {
  const selected = useWorkbenchStore((state) => state.selectedResource);
  const listings = useWorkbenchStore((state) => state.directoryListings);
  const expanded = useWorkbenchStore((state) => state.expandedDirectories);
  const loadDirectory = useWorkbenchStore((state) => state.loadDirectory);
  const toggleDirectory = useWorkbenchStore((state) => state.toggleDirectory);
  const selectDirectory = useWorkbenchStore((state) => state.selectDirectory);
  const openFile = useWorkbenchStore((state) => state.openFile);
  const selectRoot = useWorkbenchStore((state) => state.selectRoot);
  const git = useRootGitDecorations(root.id);
  const rootGitDecoration = git?.ui.gitDecorationAt(git.decorations, "", true);
  const selectedRow = useRef<HTMLButtonElement>(null);
  const rootResource = useMemo(() => childResource(root.id), [root.id]);
  const contextMenu = useContext(ExplorerContextMenuContext);
  // Like VS Code, a right-click selects what it is on (without opening a
  // File) and opens the actions for it there.
  const openContextMenu = useCallback(
    (resource: ResourceRef, row: HTMLElement, point?: PopoverPoint): void => {
      const state = useWorkbenchStore.getState();
      if (!contextMenu || !state.project || state.projectTransitioning) {
        return;
      }
      // The root heading selects as its click does (which also retries a
      // listing that failed); a row without opening its File.
      if (resource.relativePath === "") state.selectRoot(resource.rootId);
      else state.selectDirectory(resource);
      // Selection is refused while a rename or recycle runs: the menu would
      // then show the actions of what was selected before.
      if (
        !sameResource(useWorkbenchStore.getState().selectedResource, resource)
      ) {
        return;
      }
      contextMenu.open(row, point);
    },
    [contextMenu],
  );
  const rootSelected = sameResource(rootResource, selected);
  const listing = listings[resourceKey(rootResource)];
  const nestedTruncatedResource = tree.entries.find((entry) => {
    const entryListing = listings[resourceKey(entry.resource)];
    return (
      entry.kind === "directory" &&
      expanded[resourceKey(entry.resource)] &&
      entryListing?.phase === "ready" &&
      entryListing.value.truncated
    );
  })?.resource;
  const truncatedResource =
    listing?.phase === "ready" && listing.value.truncated
      ? rootResource
      : (nestedTruncatedResource ?? rootResource);

  useEffect(() => {
    selectedRow.current?.scrollIntoView({ block: "nearest" });
  }, [selected, tree.entries.length]);

  return (
    <section className="root-tree">
      <button
        className={`root-tree__heading${
          rootGitDecoration
            ? ` root-tree__heading--git git-tone--${rootGitDecoration.tone}`
            : ""
        }`}
        data-resource-key={resourceKey(rootResource)}
        type="button"
        aria-current={rootSelected ? "page" : undefined}
        aria-label={
          rootGitDecoration
            ? `${root.name}, ${rootGitDecoration.label}`
            : undefined
        }
        title={
          rootGitDecoration
            ? `${root.name}, ${rootGitDecoration.label}`
            : root.name
        }
        onClick={() => selectRoot(root.id)}
        onContextMenu={(event) => {
          event.preventDefault();
          openContextMenu(rootResource, event.currentTarget, {
            x: event.clientX,
            y: event.clientY,
          });
        }}
        onKeyDown={(event) => {
          if (!isContextMenuKey(event)) return;
          event.preventDefault();
          openContextMenu(rootResource, event.currentTarget);
        }}
      >
        <FileKindIcon path={root.name} entryKind="directory" />
        <span>{root.name}</span>
        <span className={`root-state root-state--${root.availability}`}>
          {root.availability === "available" ? "" : root.availability}
        </span>
        {git && rootGitDecoration ? (
          <git.ui.GitDecorationBadge decoration={rootGitDecoration} />
        ) : null}
      </button>
      {root.availability !== "available" ? (
        <p className="sidebar__empty">{root.message ?? "Root unavailable"}</p>
      ) : null}
      {listing?.phase === "loading" ? (
        <p className="sidebar__empty">Loading files…</p>
      ) : null}
      {listing?.phase === "error" ? (
        <p className="sidebar__empty">{listing.message}</p>
      ) : null}
      {tree.entries.map((entry) => {
        const isSelected = sameResource(entry.resource, selected);
        const key = resourceKey(entry.resource);
        return (
          <ExplorerRow
            key={key}
            ref={isSelected ? selectedRow : undefined}
            entry={entry}
            selected={isSelected}
            expanded={Boolean(expanded[key])}
            gitDecoration={git?.ui.gitDecorationAt(
              git.decorations,
              entry.resource.relativePath,
              entry.kind === "directory",
            )}
            GitBadge={git?.ui.GitDecorationBadge}
            onSelectDirectory={selectDirectory}
            onToggleDirectory={toggleDirectory}
            onOpenFile={openFile}
            onContextMenu={openContextMenu}
          />
        );
      })}
      {tree.truncated ? (
        <p className="tree__limit">
          More entries are hidden by the view limit.{" "}
          <button
            type="button"
            onClick={() => void loadDirectory(truncatedResource, true)}
          >
            Refresh folder
          </button>
        </p>
      ) : null}
    </section>
  );
});

export function ExplorerSurface() {
  const project = useWorkbenchStore(selectOpenProject);
  const listings = useWorkbenchStore((state) => state.directoryListings);
  const expanded = useWorkbenchStore((state) => state.expandedDirectories);
  const hideIgnored = useEditorSettingsStore(
    (store) => store.settings["explorer.showIgnored"] === "hidden",
  );
  const filesExclude = useEditorSettingsStore(
    (store) => store.settings["files.exclude"],
  );
  const [matcher, setMatcher] = useState(matcherModule);
  useEffect(() => {
    if (!matcher) {
      void matcherLoaded.then((module) => module && setMatcher(module));
    }
  }, [matcher]);
  const trees = useMemo(() => {
    let remaining = EXPLORER_ROW_LIMIT;
    const excluded = matcher?.pathGlobMatcher(filesExclude);
    return project?.roots.map((root) => {
      const tree = flattenFileTree(
        childResource(root.id),
        listings,
        expanded,
        remaining,
        hideIgnored,
        excluded,
      );
      remaining -= tree.entries.length;
      return { root, tree };
    });
  }, [project, listings, expanded, hideIgnored, matcher, filesExclude]);
  const fileCreateNotice = useWorkbenchStore((state) => state.fileCreateNotice);
  const folderCreateNotice = useWorkbenchStore(
    (state) => state.folderCreateNotice,
  );
  const fileRenameNotice = useWorkbenchStore((state) => state.fileRenameNotice);
  const resourceTrashNotice = useWorkbenchStore(
    (state) => state.resourceTrashNotice,
  );
  const [contextMenu] = useState(createExplorerContextMenu);
  return (
    <ExplorerContextMenuContext.Provider value={contextMenu}>
      <div className="sidebar__heading">
        <span>Explorer</span>
        <ExplorerActions />
      </div>
      {fileCreateNotice ? (
        <p className="tree__limit" role="status">
          {fileCreateNotice}
        </p>
      ) : null}
      {folderCreateNotice ? (
        <p className="tree__limit" role="status">
          {folderCreateNotice}
        </p>
      ) : null}
      {fileRenameNotice ? (
        <p className="tree__limit" role="status">
          {fileRenameNotice}
        </p>
      ) : null}
      {resourceTrashNotice ? (
        <p className="tree__limit" role="status">
          {resourceTrashNotice}
        </p>
      ) : null}
      <div className="tree" role="tree" aria-label="Project files">
        {trees?.length ? (
          trees.map(({ root, tree }) => (
            <RootTree key={root.id} root={root} tree={tree} />
          ))
        ) : (
          <p className="sidebar__empty">Open a folder to see files.</p>
        )}
      </div>
    </ExplorerContextMenuContext.Provider>
  );
}
