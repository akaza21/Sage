import { useMemo } from "react";

import { useWorkbenchStore } from "../../app/store";
import { hasActiveAcpTurn } from "../../app/acpProjectionRegistry";
import { selectProjectSwitchBlocked } from "../../app/workbenchSelectors";
import { Folder } from "../../ui/leanIcons";
import { SearchablePicker } from "../../ui/SearchablePicker";
import type { MenuPlacement } from "../../ui/menuPopoverPosition";
import "./project-scope.css";

type ProjectScopeVariant = "inline" | "caption" | "menuItem";

const variantClass: Record<ProjectScopeVariant, string> = {
  inline: "project-scope project-scope--inline",
  caption: "project-scope project-scope--caption",
  menuItem: "project-scope project-scope--menu-item",
};

export function ProjectScopeChip({
  projectName,
  disabled = false,
  open,
  onOpenChange,
  variant = "inline",
  popoverPlacement,
}: {
  projectName: string;
  disabled?: boolean;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  variant?: ProjectScopeVariant;
  popoverPlacement?: MenuPlacement;
}) {
  const menuItem = variant === "menuItem";
  const caption = variant === "caption";
  const icon = caption ? undefined : (
    <Folder className="project-scope__icon" size={14} aria-hidden="true" />
  );
  const projects = useWorkbenchStore((state) => state.projects);
  const projectId = useWorkbenchStore((state) => state.project?.id ?? null);
  const switchProject = useWorkbenchStore((state) => state.switchProject);
  const switchBlocked = useWorkbenchStore(selectProjectSwitchBlocked);
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
      role={candidates.length === 0 ? "img" : undefined}
      aria-label={
        candidates.length === 0 ? `Project: ${projectName}` : undefined
      }
      title={
        candidates.length === 0
          ? `Project: ${projectName} (No other projects available)`
          : undefined
      }
      className={variantClass[variant]}
    >
      {candidates.length > 0 ? (
        <SearchablePicker
          triggerRole={menuItem ? "menuitem" : undefined}
          className="project-scope__picker"
          appearance="plain"
          popoverAppearance="menu"
          triggerClassName={
            menuItem ? "menu-item composer__add-action" : undefined
          }
          chevron={false}
          portalTarget={menuItem ? "closest-popover" : "closest-dialog"}
          popoverClassName="composer__popover"
          popoverPlacement={popoverPlacement}
          ariaLabel={menuItem ? "Work in a project" : `Project: ${projectName}`}
          stateTitle={`Project: ${projectName}`}
          triggerIcon={icon}
          triggerLabel={
            menuItem ? (
              "Work in a project"
            ) : caption ? (
              <span className="project-scope__caption">{projectName}</span>
            ) : undefined
          }
          triggerDetail={
            menuItem ? (
              <span className="composer__action-description">
                Choose project for new chats
              </span>
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
          {icon}
          <span
            className={
              caption
                ? "project-scope__label project-scope__caption"
                : "project-scope__label"
            }
            title={`Project: ${projectName}`}
          >
            {projectName}
          </span>
        </>
      )}
    </span>
  );
}
