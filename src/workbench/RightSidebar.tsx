import {
  CircleAlert,
  Eye,
  FileSearch,
  Files,
  GitBranch,
  ListTree,
} from "lucide-react";
import { useEffect, useLayoutEffect, useRef, type KeyboardEvent } from "react";

import type { SidebarView } from "../app/store";
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
  { id: "preview", label: "Preview", icon: Eye },
  { id: "problems", label: "Problems", icon: CircleAlert },
  { id: "outline", label: "Outline", icon: ListTree },
];

const sidebarPanelId = "project-tools-panel";

function SidebarContent({ view }: { view: SidebarView }) {
  return renderRegisteredTool(view);
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
