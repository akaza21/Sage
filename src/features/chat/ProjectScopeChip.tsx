import { useMemo } from "react";

import { useWorkbenchStore } from "../../app/store";
import { hasActiveAcpTurn } from "../../app/acpProjectionRegistry";
import { Folder } from "../../ui/leanIcons";
import { SearchablePicker } from "../../ui/SearchablePicker";
import type { MenuPlacement } from "../../ui/menuPopoverPosition";

export function ProjectScopeChip({
  projectName,
  disabled = false,
  open,
  onOpenChange,
  menuItem = false,
  popoverPlacement,
}: {
  projectName: string;
  disabled?: boolean;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  menuItem?: boolean;
  popoverPlacement?: MenuPlacement;
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
  // The "+" menu offers no row without projects to choose from.
  if (menuItem && candidates.length === 0) return null;
  return (
    <span
      role={candidates.length === 0 ? "img" : undefined}
      aria-label={
        candidates.length === 0 ? `Project: ${projectName}` : undefined
      }
      title={
        candidates.length === 0
          ? `Project: ${projectName} (No other projects available)`
          : undefined
      }
      className={
        candidates.length > 0
          ? "select-control select-control--chip project-scope"
          : "project-scope"
      }
    >
      {candidates.length > 0 ? (
        <SearchablePicker
          triggerRole={menuItem ? "menuitem" : undefined}
          triggerClassName={menuItem ? "menu-item" : undefined}
          portalTarget={menuItem ? "closest-popover" : "closest-dialog"}
          popoverClassName="composer__popover composer__project-menu"
          popoverPlacement={popoverPlacement}
          ariaLabel={menuItem ? "Work in a project" : `Project: ${projectName}`}
          stateTitle={`Project: ${projectName}`}
          triggerIcon={
            <Folder
              className="project-scope__icon"
              size={14}
              aria-hidden="true"
            />
          }
          triggerLabel={
            menuItem ? (
              <>
                Work in a project
                <span className="composer__action-description">
                  Choose project for new chats
                </span>
              </>
            ) : undefined
          }
          open={open}
          onOpenChange={onOpenChange}
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
        <>
          <Folder
            className="project-scope__icon"
            size={14}
            aria-hidden="true"
          />
          <span
            className="project-scope__label"
            title={`Project: ${projectName}`}
          >
            {projectName}
          </span>
        </>
      )}
    </span>
  );
}
