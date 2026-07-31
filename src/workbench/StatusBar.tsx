import { GitBranch, PanelRightClose, PanelRightOpen } from "lucide-react";

import { useWorkbenchStore } from "../app/store";

export function StatusBar() {
  const sidebarOpen = useWorkbenchStore((state) => state.sidebarOpen);
  const toggleSidebar = useWorkbenchStore((state) => state.toggleSidebar);
  const repositories = useWorkbenchStore((state) => state.repositories);
  const recoveryNotice = useWorkbenchStore((state) => state.recoveryNotice);
  const branch = repositories.roots.find(
    (root) => root.state === "repository",
  )?.branch;

  return (
    <footer className="status-bar">
      <div className="status-bar__group">
        <span>
          <GitBranch size={13} aria-hidden="true" />
          {branch ??
            (repositories.roots.length === 0 ? "No project" : "No repository")}
        </span>
        <span role={recoveryNotice ? "status" : undefined}>
          {recoveryNotice ?? "0 problems"}
        </span>
      </div>
      <div className="status-bar__group">
        <span>1 session</span>
        <span>Trusted project</span>
        <button
          className="status-bar__button"
          type="button"
          aria-label={sidebarOpen ? "Hide right sidebar" : "Show right sidebar"}
          onClick={toggleSidebar}
        >
          {sidebarOpen ? (
            <PanelRightClose size={14} aria-hidden="true" />
          ) : (
            <PanelRightOpen size={14} aria-hidden="true" />
          )}
        </button>
      </div>
    </footer>
  );
}
