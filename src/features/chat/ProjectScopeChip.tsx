import { useMemo } from "react";

import { useWorkbenchStore } from "../../app/store";
import { hasActiveAcpTurn } from "../../app/acpProjectionRegistry";
import { Folder } from "../../ui/leanIcons";
import { SearchablePicker } from "../../ui/SearchablePicker";

export function ProjectScopeChip({
  projectName,
  disabled = false,
}: {
  projectName: string;
  disabled?: boolean;
}) {
  const projects = useWorkbenchStore((state) => state.projects);
  const projectId = useWorkbenchStore((state) => state.project?.id ?? null);
  const switchProject = useWorkbenchStore((state) => state.switchProject);
  const switchBlocked = useWorkbenchStore(
    (state) =>
      state.projectTransitioning ||
      state.sending ||
      Object.values(state.chatUiById).some(
        (chat) => chat.sending || chat.modeChanging,
      ),
  );
  const candidates = useMemo(
    () =>
      projects.map((project) => ({
        id: project.id,
        label: project.id === projectId ? projectName : project.name,
      })),
    [projects, projectId, projectName],
  );
  return (
    <span
      title={
        candidates.length === 0 ? "No other projects available" : undefined
      }
      className={
        candidates.length > 0
          ? "select-control select-control--chip project-scope"
          : "project-scope"
      }
    >
      <Folder className="project-scope__icon" size={14} aria-hidden="true" />
      {candidates.length > 0 ? (
        <SearchablePicker
          portalTarget="closest-dialog"
          ariaLabel="Project"
          candidates={candidates}
          disabled={disabled || switchBlocked}
          emptyLabel={projectName}
          onSelect={(id) => {
            const state = useWorkbenchStore.getState();
            const activeChatId =
              state.tabs.find(
                (tab) => tab.id === state.activeTabId && tab.kind === "chat",
              )?.id ?? null;
            if (id !== state.project?.id && hasActiveAcpTurn(activeChatId)) {
              useWorkbenchStore.setState({
                error: "Finish the active turn before switching projects.",
              });
              return;
            }
            void switchProject(id);
          }}
          selectedId={projectId}
        />
      ) : (
        <span>{projectName}</span>
      )}
    </span>
  );
}
