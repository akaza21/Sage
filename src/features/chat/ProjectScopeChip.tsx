import { useMemo, useSyncExternalStore } from "react";

import { useWorkbenchStore } from "../../app/store";
import {
  hasActiveAcpTurn,
  subscribeAcpTurnActivity,
} from "../../app/acpProjectionRegistry";
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
  const projectGeneration = useWorkbenchStore(
    (state) => state.projectGeneration,
  );
  const turnBusy = useSyncExternalStore(subscribeAcpTurnActivity, () =>
    hasActiveAcpTurn(projectId, projectGeneration),
  );
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
          ariaLabel="Project"
          candidates={candidates}
          disabled={disabled || switchBlocked || turnBusy}
          emptyLabel={projectName}
          onSelect={(id) => void switchProject(id)}
          selectedId={projectId}
        />
      ) : (
        <span>{projectName}</span>
      )}
    </span>
  );
}
