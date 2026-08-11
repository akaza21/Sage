import { GitBranch, PanelRightClose, PanelRightOpen } from "lucide-react";

import { useWorkbenchStore } from "../app/store";
import type { ProjectRepositoryStatus } from "../domain/workbench";

function repositoryStatusLabel(repositories: ProjectRepositoryStatus) {
  if (repositories.roots.length === 0) return "No project";
  if (repositories.roots.length > 1) {
    const count = repositories.roots.filter(
      (root) => root.state === "repository",
    ).length;
    return `${count} ${count === 1 ? "repository" : "repositories"} across ${repositories.roots.length} roots`;
  }

  const [repository] = repositories.roots;
  if (repository.state === "repository") {
    return repository.branch ?? "Repository";
  }
  return repository.state === "notRepository"
    ? "No repository"
    : "Repository unavailable";
}

export function StatusBar() {
  const sidebarOpen = useWorkbenchStore((state) => state.sidebarOpen);
  const toggleSidebar = useWorkbenchStore((state) => state.toggleSidebar);
  const repositories = useWorkbenchStore((state) => state.repositories);
  const recoveryNotice = useWorkbenchStore((state) => state.recoveryNotice);

  return (
    <footer className="status-bar">
      <div className="status-bar__group">
        <span>
          <GitBranch size={13} aria-hidden="true" />
          {repositoryStatusLabel(repositories)}
        </span>
        {recoveryNotice ? <span role="status">{recoveryNotice}</span> : null}
      </div>
      <div className="status-bar__group">
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
