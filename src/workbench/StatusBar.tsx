import { lazy, Suspense, type ReactNode } from "react";
import { PanelLeftClose, PanelLeftOpen } from "../ui/leanIcons";

import { useWorkbenchStore } from "../app/store";
import { EditorStatusItems } from "./EditorStatusItems";

const SourceBranchStatus = lazy(() => import("./StatusBarDetails"));

export function StatusBar({
  inert = false,
  railLeading = false,
  showSidebarToggle = true,
  showEditorSettings = true,
  terminalControls,
}: {
  inert?: boolean;
  railLeading?: boolean;
  showSidebarToggle?: boolean;
  showEditorSettings?: boolean;
  terminalControls?: ReactNode;
}) {
  const sidebarOpen = useWorkbenchStore((state) => state.sidebarOpen);
  const toggleSidebar = useWorkbenchStore((state) => state.toggleSidebar);
  const recoveryNotice = useWorkbenchStore((state) => state.recoveryNotice);
  const browserClearNotice = useWorkbenchStore(
    (state) => state.browserClearNotice,
  );
  const sidebarLabel = `${sidebarOpen ? "Hide" : "Show"} ${railLeading ? "left sidebar" : "artifact pane"}`;

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
        {recoveryNotice ? <span role="status">{recoveryNotice}</span> : null}
        {browserClearNotice ? (
          <span role="status">{browserClearNotice}</span>
        ) : null}
      </div>
      <div className="status-bar__group">
        {terminalControls}
        {showEditorSettings ? <EditorStatusItems /> : null}
        {showSidebarToggle ? (
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
        ) : null}
      </div>
    </footer>
  );
}
