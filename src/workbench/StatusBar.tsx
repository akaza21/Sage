import { lazy, Suspense, type ReactNode } from "react";
import { PanelLeftClose, PanelLeftOpen } from "../ui/leanIcons";

import { useWorkbenchStore } from "../app/store";
import { EditorStatusItems } from "./EditorStatusItems";

const SourceBranchStatus = lazy(() => import("./StatusBarDetails"));

/** Recovery and browser-data notices; the status bar shows them in Editor,
 *  the toolbar row in Agent. */
export function WorkbenchNotices() {
  const recoveryNotice = useWorkbenchStore((state) => state.recoveryNotice);
  const browserClearNotice = useWorkbenchStore(
    (state) => state.browserClearNotice,
  );
  return (
    <>
      {recoveryNotice ? <span role="status">{recoveryNotice}</span> : null}
      {browserClearNotice ? (
        <span role="status">{browserClearNotice}</span>
      ) : null}
    </>
  );
}

/** The Editor status bar. Agent mode has none. */
export function StatusBar({
  inert = false,
  terminalControls,
}: {
  inert?: boolean;
  terminalControls?: ReactNode;
}) {
  const sidebarOpen = useWorkbenchStore((state) => state.sidebarOpen);
  const toggleSidebar = useWorkbenchStore((state) => state.toggleSidebar);
  const sidebarLabel = `${sidebarOpen ? "Hide" : "Show"} left sidebar`;

  return (
    <footer
      className="status-bar"
      inert={inert}
      aria-hidden={inert || undefined}
    >
      <div className="status-bar__group">
        <Suspense fallback={null}>
          <SourceBranchStatus />
        </Suspense>
        <WorkbenchNotices />
      </div>
      <div className="status-bar__group">
        {terminalControls}
        <EditorStatusItems />
        <button
          className="status-bar__button"
          type="button"
          aria-label={sidebarLabel}
          onClick={toggleSidebar}
        >
          {sidebarOpen ? (
            <PanelLeftClose size={14} aria-hidden="true" />
          ) : (
            <PanelLeftOpen size={14} aria-hidden="true" />
          )}
        </button>
      </div>
    </footer>
  );
}
