import {
  FileSearch,
  Files,
  GitBranch,
  ListTree,
  Settings,
} from "../ui/leanIcons";
import { useEffect, useLayoutEffect, useRef, type KeyboardEvent } from "react";

import type { SidebarView, WorkbenchState } from "../app/store";
import { useWorkbenchStore } from "../app/store";
import { renderRegisteredTool } from "./surfaceRegistry";
import { registerWorkbenchSurfaces } from "./registeredSurfaces";

registerWorkbenchSurfaces();

const views: Array<{
  id: SidebarView;
  label: string;
  icon: typeof Files;
}> = [
  { id: "explorer", label: "Explorer", icon: Files },
  { id: "search", label: "Search", icon: FileSearch },
  { id: "sourceControl", label: "Source Control", icon: GitBranch },
  { id: "outline", label: "Outline", icon: ListTree },
];

const sidebarPanelId = "project-tools-panel";

/** VS Code's Source Control count: every listed change across the roots and
 * the repositories found one level down, with a plus when Git reported more
 * than a bounded list holds. */
function pendingChangesBadge({
  repositories,
  repositorySummaries,
}: WorkbenchState) {
  let count = 0;
  let more = false;
  for (const root of repositories.roots) {
    count += root.changes.length;
    more ||= root.truncated || root.changesOverflowed === true;
  }
  for (const summary of repositorySummaries) {
    count += summary.changeCount;
    more ||= summary.changesOverflowed;
  }
  return count ? `${count}${more ? "+" : ""}` : null;
}

function SidebarContent({ view }: { view: SidebarView }) {
  return renderRegisteredTool(view);
}

export function RightSidebar({
  compact,
  railFirst = false,
}: {
  compact: boolean;
  railFirst?: boolean;
}) {
  const sidebarOpen = useWorkbenchStore((state) => state.sidebarOpen);
  const sidebarView = useWorkbenchStore((state) => state.sidebarView);
  const setSidebarView = useWorkbenchStore((state) => state.setSidebarView);
  const toggleSidebar = useWorkbenchStore((state) => state.toggleSidebar);
  const openSettings = useWorkbenchStore((state) => state.openSettings);
  const changesBadge = useWorkbenchStore(pendingChangesBadge);
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
    if (sidebarOpen && sidebarView === id) {
      // As in VS Code, the shown view's item hides the docked side bar.
      if (compact) panelRef.current?.focus();
      else toggleSidebar();
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

  const panel = sidebarOpen ? (
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
  ) : null;
  const rail = (
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
          title={
            id === "sourceControl" && changesBadge
              ? `${label} (${changesBadge} pending changes)`
              : label
          }
          onClick={() => selectView(id)}
        >
          <Icon size={24} strokeWidth={1.5} aria-hidden="true" />
          {id === "sourceControl" && changesBadge ? (
            <span className="activity-rail__badge" aria-hidden="true">
              {changesBadge}
            </span>
          ) : null}
        </button>
      ))}
      <button
        className="activity-rail__button activity-rail__button--end"
        type="button"
        aria-label="Editor settings"
        title="Editor settings"
        onClick={() => void openSettings()}
      >
        <Settings size={24} strokeWidth={1.5} aria-hidden="true" />
      </button>
    </nav>
  );

  return (
    <aside
      className={`right-region ${sidebarOpen ? "" : "right-region--collapsed"}${
        railFirst ? " right-region--leading" : ""
      }`}
      aria-label="Project tools"
      onKeyDown={handleDrawerKeyDown}
    >
      {railFirst ? (
        <>
          {rail}
          {panel}
        </>
      ) : (
        <>
          {panel}
          {rail}
        </>
      )}
    </aside>
  );
}
