import {
  Braces,
  ChevronRight,
  FileCode2,
  FileSearch,
  Files,
  Folder,
  GitBranch,
  Link2,
  ListChecks,
} from "lucide-react";
import { useEffect, useLayoutEffect, useRef, type KeyboardEvent } from "react";

import type { SidebarView } from "../app/store";
import { useWorkbenchStore } from "../app/store";
import {
  childResource,
  resourceKey,
  sameResource,
} from "../domain/resourceIdentity";
import type { ProjectRootSummary } from "../domain/workbench";
import { flattenFileTree } from "../features/files/fileTree";
import { NewFileControl } from "../features/files/NewFileControl";
import { explorerCreateParent } from "../features/files/newFileContract";
import { WorkspaceSearchView } from "../features/search";
import { SourceControlView } from "./SourceControlView";

const views: Array<{
  id: SidebarView;
  label: string;
  icon: typeof Files;
}> = [
  { id: "explorer", label: "Explorer", icon: Files },
  { id: "search", label: "Search", icon: FileSearch },
  { id: "sourceControl", label: "Source Control", icon: GitBranch },
  { id: "problems", label: "Problems", icon: ListChecks },
  { id: "outline", label: "Outline", icon: Braces },
];

const sidebarPanelId = "project-tools-panel";

function RootTree({ root }: { root: ProjectRootSummary }) {
  const selected = useWorkbenchStore((state) => state.selectedResource);
  const listings = useWorkbenchStore((state) => state.directoryListings);
  const expanded = useWorkbenchStore((state) => state.expandedDirectories);
  const toggleDirectory = useWorkbenchStore((state) => state.toggleDirectory);
  const selectDirectory = useWorkbenchStore((state) => state.selectDirectory);
  const openFile = useWorkbenchStore((state) => state.openFile);
  const selectRoot = useWorkbenchStore((state) => state.selectRoot);
  const selectedRow = useRef<HTMLButtonElement>(null);
  const rootResource = childResource(root.id);
  const tree = flattenFileTree(rootResource, listings, expanded);
  const listing = listings[resourceKey(rootResource)];

  useEffect(() => {
    selectedRow.current?.scrollIntoView({ block: "nearest" });
  }, [selected, tree.entries.length]);

  return (
    <section className="root-tree">
      <button
        className="root-tree__heading"
        type="button"
        onClick={() => selectRoot(root.id)}
      >
        <Folder size={14} aria-hidden="true" />
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
        const directory = entry.kind === "directory";
        const file = entry.kind === "file";
        const Icon =
          entry.kind === "symlink" ? Link2 : directory ? Folder : FileCode2;
        return (
          <button
            className={`tree__row ${directory ? "tree__row--folder" : ""} ${
              isSelected ? "tree__row--selected" : ""
            }`}
            key={key}
            ref={isSelected ? selectedRow : undefined}
            type="button"
            role="treeitem"
            aria-label={
              directory
                ? `Toggle folder ${entry.resource.relativePath}`
                : file
                  ? `Open file ${entry.resource.relativePath}`
                  : `Unavailable resource ${entry.resource.relativePath}`
            }
            aria-current={isSelected ? "page" : undefined}
            aria-expanded={directory ? Boolean(expanded[key]) : undefined}
            style={{ paddingLeft: 9 + entry.depth * 16 }}
            disabled={!directory && !file}
            onClick={() => {
              if (directory) {
                selectDirectory(entry.resource);
                toggleDirectory(entry.resource);
              } else {
                void openFile(entry.resource);
              }
            }}
          >
            {directory ? (
              <ChevronRight
                className={
                  expanded[key] ? "tree__chevron--expanded" : undefined
                }
                size={13}
                aria-hidden="true"
              />
            ) : (
              <span className="tree__indent" />
            )}
            <Icon size={14} strokeWidth={1.7} aria-hidden="true" />
            <span>{entry.name}</span>
          </button>
        );
      })}
      {tree.truncated ? (
        <p className="tree__limit">
          More entries are hidden by the view limit.
        </p>
      ) : null}
    </section>
  );
}

function ExplorerView() {
  const project = useWorkbenchStore((state) => state.project);
  const selectedRootId = useWorkbenchStore((state) => state.selectedRootId);
  const selectedResource = useWorkbenchStore((state) => state.selectedResource);
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const listings = useWorkbenchStore((state) => state.directoryListings);
  const fileCreateNotice = useWorkbenchStore((state) => state.fileCreateNotice);
  const parent = project
    ? explorerCreateParent(
        project,
        selectedRootId,
        selectedResource,
        tabs,
        activeTabId,
        listings,
      )
    : null;
  const rootName =
    project?.roots.find((root) => root.id === parent?.rootId)?.name ?? null;
  return (
    <>
      <div className="sidebar__heading">
        <span>Explorer</span>
        <span className="sidebar__heading-actions">
          <span className="muted">{project?.name ?? "No project"}</span>
          <NewFileControl parent={parent} rootName={rootName} />
        </span>
      </div>
      {fileCreateNotice ? (
        <p className="tree__limit" role="status">
          {fileCreateNotice}
        </p>
      ) : null}
      <div className="tree" role="tree" aria-label="Project files">
        {project?.roots.map((root) => (
          <RootTree key={root.id} root={root} />
        ))}
      </div>
    </>
  );
}

function SearchView() {
  const projectId = useWorkbenchStore((state) => state.project?.id ?? null);
  const transitionEpoch = useWorkbenchStore(
    (state) => state.nativeTransitionEpoch,
  );
  const openSearchMatch = useWorkbenchStore((state) => state.openSearchMatch);

  return (
    <WorkspaceSearchView
      projectId={projectId}
      transitionEpoch={transitionEpoch}
      onOpenMatch={(request) => void openSearchMatch(request)}
    />
  );
}

function SidebarContent({ view }: { view: SidebarView }) {
  if (view === "explorer") return <ExplorerView />;
  if (view === "search") return <SearchView />;
  if (view === "sourceControl") return <SourceControlView />;
  const labels = {
    problems: ["Problems", "No problems have been reported."],
    outline: ["Outline", "Open a source file to view its outline."],
  } as const;
  const [title, body] = labels[view];
  return (
    <>
      <div className="sidebar__heading">{title}</div>
      <p className="sidebar__empty">{body}</p>
    </>
  );
}

export function RightSidebar({ compact }: { compact: boolean }) {
  const sidebarOpen = useWorkbenchStore((state) => state.sidebarOpen);
  const sidebarView = useWorkbenchStore((state) => state.sidebarView);
  const setSidebarView = useWorkbenchStore((state) => state.setSidebarView);
  const toggleSidebar = useWorkbenchStore((state) => state.toggleSidebar);
  const panelRef = useRef<HTMLElement>(null);
  const railButtonRefs = useRef(new Map<SidebarView, HTMLButtonElement>());
  const restoreRailFocus = useRef<SidebarView | null>(null);
  const activeLabel =
    views.find((view) => view.id === sidebarView)?.label ?? "Project";

  useLayoutEffect(() => {
    const panel = panelRef.current;
    if (
      compact &&
      sidebarOpen &&
      panel &&
      !panel.contains(document.activeElement)
    ) {
      panel.focus();
    }
  }, [compact, sidebarOpen, sidebarView]);

  useEffect(() => {
    const view = restoreRailFocus.current;
    if (sidebarOpen || !view) return;
    restoreRailFocus.current = null;
    railButtonRefs.current.get(view)?.focus();
  }, [sidebarOpen]);

  function selectView(id: SidebarView) {
    if (compact && sidebarOpen && sidebarView === id) {
      panelRef.current?.focus();
      return;
    }
    setSidebarView(id);
  }

  function handleDrawerKeyDown(event: KeyboardEvent<HTMLElement>) {
    if (!compact || !sidebarOpen || event.key !== "Escape") return;
    event.preventDefault();
    restoreRailFocus.current = sidebarView;
    toggleSidebar();
  }

  return (
    <aside
      className={`right-region ${sidebarOpen ? "" : "right-region--collapsed"}`}
      aria-label="Project tools"
      onKeyDown={handleDrawerKeyDown}
    >
      {sidebarOpen ? (
        <section
          ref={panelRef}
          id={sidebarPanelId}
          className="sidebar"
          role={compact ? "dialog" : undefined}
          aria-label={`${activeLabel} project tools`}
          tabIndex={compact ? -1 : undefined}
        >
          <SidebarContent view={sidebarView} />
        </section>
      ) : null}
      <nav className="activity-rail" aria-label="Activity">
        {views.map(({ id, label, icon: Icon }) => (
          <button
            className={`activity-rail__button ${
              sidebarOpen && sidebarView === id
                ? "activity-rail__button--active"
                : ""
            }`}
            key={id}
            ref={(element) => {
              if (element) railButtonRefs.current.set(id, element);
              else railButtonRefs.current.delete(id);
            }}
            type="button"
            aria-label={label}
            aria-controls={sidebarPanelId}
            aria-expanded={sidebarOpen && sidebarView === id}
            title={label}
            onClick={() => selectView(id)}
          >
            <Icon size={19} strokeWidth={1.7} aria-hidden="true" />
          </button>
        ))}
      </nav>
    </aside>
  );
}
