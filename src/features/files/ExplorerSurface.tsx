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
import { ExplorerActions } from "./ExplorerActions";
import {
  ExplorerContextMenuContext,
  createExplorerContextMenu,
} from "./explorerContextMenu";
import { ExplorerRow } from "./ExplorerRow";
import { FileKindIcon } from "./FileKindIcon";
import { flattenFileTree } from "./fileTree";

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
}: {
  root: ProjectRootSummary;
}) {
  const selected = useWorkbenchStore((state) => state.selectedResource);
  const listings = useWorkbenchStore((state) => state.directoryListings);
  const expanded = useWorkbenchStore((state) => state.expandedDirectories);
  const loadDirectory = useWorkbenchStore((state) => state.loadDirectory);
  const toggleDirectory = useWorkbenchStore((state) => state.toggleDirectory);
  const selectDirectory = useWorkbenchStore((state) => state.selectDirectory);
  const openFile = useWorkbenchStore((state) => state.openFile);
  const selectRoot = useWorkbenchStore((state) => state.selectRoot);
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
  const selectedRow = useRef<HTMLButtonElement>(null);
  const rootResource = useMemo(() => childResource(root.id), [root.id]);
  const contextMenu = useContext(ExplorerContextMenuContext);
  // Like VS Code, a right-click selects what it is on (without opening a
  // File) and opens the actions for it there.
  const openContextMenu = useCallback(
    (resource: ResourceRef, row: HTMLElement) => {
      const state = useWorkbenchStore.getState();
      if (!contextMenu || !state.project || state.projectTransitioning) {
        return false;
      }
      state.selectDirectory(resource);
      // Selection is refused while a rename or recycle runs: the menu would
      // then show the actions of what was selected before.
      if (
        !sameResource(useWorkbenchStore.getState().selectedResource, resource)
      ) {
        return false;
      }
      return contextMenu.open(row);
    },
    [contextMenu],
  );
  const rootSelected = sameResource(rootResource, selected);
  const tree = useMemo(
    () =>
      flattenFileTree(
        rootResource,
        listings,
        expanded,
        500,
        hideIgnored,
        matcher?.pathGlobMatcher(filesExclude),
      ),
    [expanded, filesExclude, hideIgnored, listings, matcher, rootResource],
  );
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
        className="root-tree__heading"
        data-resource-key={resourceKey(rootResource)}
        type="button"
        aria-current={rootSelected ? "page" : undefined}
        title={root.name}
        onClick={() => selectRoot(root.id)}
        onContextMenu={(event) => {
          if (openContextMenu(rootResource, event.currentTarget)) {
            event.preventDefault();
          }
        }}
      >
        <FileKindIcon path={root.name} entryKind="directory" />
        <span>{root.name}</span>
        <span className={`root-state root-state--${root.availability}`}>
          {root.availability === "available" ? "" : root.availability}
        </span>
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
        {project?.roots.length ? (
          project.roots.map((root) => <RootTree key={root.id} root={root} />)
        ) : (
          <p className="sidebar__empty">Open a folder to see files.</p>
        )}
      </div>
    </ExplorerContextMenuContext.Provider>
  );
}
