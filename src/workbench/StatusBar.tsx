import { GitBranch, PanelLeftClose, PanelLeftOpen } from "lucide-react";

import { useEditorAppearanceStore } from "../app/editorAppearanceStore";
import { useEditorCaretStore } from "../app/editorCaretStore";
import { useWorkbenchStore } from "../app/store";
import type { ProjectRepositoryStatus } from "../domain/workbench";
import { caretStatusLabel } from "../features/files/editorCaret";
import { editorLanguageStatusLabel } from "../features/files/editorLanguage";

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

export function StatusBar({
  railLeading = false,
  showSidebarToggle = true,
  showEditorSettings = true,
}: {
  railLeading?: boolean;
  showSidebarToggle?: boolean;
  showEditorSettings?: boolean;
}) {
  const sidebarOpen = useWorkbenchStore((state) => state.sidebarOpen);
  const toggleSidebar = useWorkbenchStore((state) => state.toggleSidebar);
  const repositories = useWorkbenchStore((state) => state.repositories);
  const recoveryNotice = useWorkbenchStore((state) => state.recoveryNotice);
  const browserClearNotice = useWorkbenchStore(
    (state) => state.browserClearNotice,
  );
  const tabSize = useEditorAppearanceStore((state) => state.tabSize);
  const caretLabel = caretStatusLabel(
    useEditorCaretStore((state) => state.caret),
  );
  const languageLabel = editorLanguageStatusLabel(
    useWorkbenchStore((state) => {
      const tab = state.tabs.find((item) => item.id === state.activeTabId);
      if (tab?.kind !== "file") {
        return null;
      }
      return tab.resource?.relativePath ?? tab.title;
    }),
  );
  const sidebarLabel = railLeading
    ? sidebarOpen
      ? "Hide left sidebar"
      : "Show left sidebar"
    : sidebarOpen
      ? "Hide artifact pane"
      : "Show artifact pane";

  return (
    <footer className="status-bar">
      <div className="status-bar__group">
        <span>
          <GitBranch size={14} aria-hidden="true" />
          {repositoryStatusLabel(repositories)}
        </span>
        {recoveryNotice ? <span role="status">{recoveryNotice}</span> : null}
        {browserClearNotice ? (
          <span role="status">{browserClearNotice}</span>
        ) : null}
      </div>
      <div className="status-bar__group">
        {showEditorSettings && caretLabel ? (
          <span title="Caret position">{caretLabel}</span>
        ) : null}
        {showEditorSettings && languageLabel ? (
          <span title="Editor language">{languageLabel}</span>
        ) : null}
        {showEditorSettings ? (
          <span title="Indentation">Spaces: {tabSize}</span>
        ) : null}
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
