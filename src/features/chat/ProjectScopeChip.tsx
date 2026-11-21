import { useMemo, useState } from "react";
import { currentProjectLayout } from "../../app/projectProjection";
import { usePinnedAgentChatOwner } from "../../app/agentChatOwner";
import { hasActiveAcpTurn } from "../../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../app/store";
import { selectProjectSwitchBlocked } from "../../app/workbenchSelectors";
import { Folder, Plus, X } from "../../ui/leanIcons";
import { SearchablePicker } from "../../ui/SearchablePicker";
import type { MenuPlacement } from "../../ui/menuPopoverPosition";
import { lazyParts } from "./lazyParts";
import type * as CreateProjectChunk from "./CreateChatProjectDialog";
import type { ChatProjectCreation } from "./CreateChatProjectDialog";
import "./project-scope.css";

const CreateProjectPart = lazyParts<typeof CreateProjectChunk>(
  () => import("./CreateChatProjectDialog"),
);
const NEW_PROJECT_ACTION = {
  id: "scope:new",
  label: "New project",
  leadingIcon: Plus,
  separatorBefore: true,
  alwaysVisible: true,
};
const NO_PROJECT_ACTION = {
  id: "scope:none",
  label: "Don't work in a project",
  leadingIcon: X,
  alwaysVisible: true,
};
type ProjectScopeVariant = "inline" | "caption" | "menuItem";

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
  const projects = useWorkbenchStore((state) => state.projects);
  const project = useWorkbenchStore((state) => state.project);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const pinnedChatId = usePinnedAgentChatOwner();
  const draft = useWorkbenchStore((state) =>
    state.agentCenter === "draft" ? state.newChatDraft : null,
  );
  const chatId =
    draft?.submission.status === "created"
      ? draft.submission.chatId
      : (pinnedChatId ?? activeTabId);
  const sessionId = useWorkbenchStore((state) => state.sessionId);
  const setProject = useWorkbenchStore((state) => state.setChatProject);
  const updateDraft = useWorkbenchStore((state) => state.updateNewChatDraft);
  const switchBlocked = useWorkbenchStore(selectProjectSwitchBlocked);
  const [creating, setCreating] = useState<ChatProjectCreation | null>(null);
  const projectId = draft
    ? draft.targetProjectId === undefined
      ? draft.projectId
      : draft.targetProjectId
    : project?.isInternal
      ? null
      : (project?.id ?? null);
  const scoped = !!projectId;
  const label = scoped
    ? draft
      ? (projects.find((project) => project.id === projectId)?.name ??
        projectName)
      : projectName
    : "Choose project";
  const triggerLabel = menuItem ? "Work in a project" : label;
  const projectTitle = `Project: ${label}`;
  const readOnly =
    disabled ||
    switchBlocked ||
    (draft
      ? draft.submission.status === "submitting" ||
        draft.submission.status === "unconfirmed"
      : !!sessionId);
  const candidates = useMemo(() => {
    const counts = new Map<string, number>();
    for (const { name } of projects) {
      counts.set(name, (counts.get(name) ?? 0) + 1);
    }
    return [
      ...projects.map(({ id, name, parentPath: parent }) => {
        const duplicate = counts.get(name)! > 1;
        return {
          id,
          leadingIcon: Folder,
          label: id === projectId ? label : name,
          subtitle: duplicate
            ? parent
              ? parent.length > 64
                ? `...${parent.slice(-61)}`
                : parent
              : "No source folder"
            : undefined,
          accessibleLabel: duplicate
            ? `${name}, ${parent ?? "No source folder"}${projects.some((other) => other.id !== id && other.name === name && other.parentPath === parent) ? `, ${id}` : ""}`
            : name,
        };
      }),
      NEW_PROJECT_ACTION,
      ...(scoped ? [NO_PROJECT_ACTION] : []),
    ];
  }, [projects, projectId, label, scoped]);
  const choose = (id: string | null) => {
    if (readOnly) return;
    if (!draft && id !== projectId && hasActiveAcpTurn(chatId)) {
      useWorkbenchStore.setState({
        error: "Finish the active turn before switching projects.",
      });
      return;
    }
    if (draft) updateDraft({ targetProjectId: id });
    else if (id !== projectId)
      void setProject(
        id,
        currentProjectLayout(useWorkbenchStore.getState(), chatId) ?? undefined,
      );
  };
  return (
    <span
      className={`project-scope project-scope--${variant === "menuItem" ? "menu-item" : variant}`}
    >
      <SearchablePicker
        triggerRole={menuItem ? "menuitem" : undefined}
        className="project-scope__picker"
        appearance="plain"
        popoverAppearance="menu"
        searchPlaceholder="Search projects"
        triggerClassName={
          menuItem ? "menu-item composer__add-action" : undefined
        }
        chevron={false}
        portalTarget={menuItem ? "closest-popover" : "closest-dialog"}
        popoverClassName="composer__popover"
        popoverPlacement={popoverPlacement}
        ariaLabel={scoped && !menuItem ? projectTitle : triggerLabel}
        stateTitle={
          readOnly ? projectTitle : "Change the project for this chat"
        }
        triggerIcon={caption ? undefined : <Folder size={14} />}
        triggerLabel={
          caption ? (
            <span className="project-scope__caption">{label}</span>
          ) : (
            triggerLabel
          )
        }
        triggerDetail={
          menuItem ? (
            <span className="composer__action-description">
              Choose project for this chat
            </span>
          ) : undefined
        }
        open={open}
        onOpenChange={onOpenChange}
        candidates={candidates}
        disabled={readOnly}
        selectedId={projectId}
        onSelect={(id) => {
          if (id !== NEW_PROJECT_ACTION.id) {
            choose(id === NO_PROJECT_ACTION.id ? null : id);
            return;
          }
          const state = useWorkbenchStore.getState();
          const composer = state.chatUiById[chatId ?? ""];
          setCreating({
            layout: currentProjectLayout(state, chatId),
            draft,
            text: draft?.text ?? composer?.composerText ?? state.composerText,
            composer,
          });
        }}
      />
      {scoped && !readOnly && !menuItem ? (
        <button
          type="button"
          className="project-scope__clear"
          aria-label="Remove project from this chat"
          title="Don't work in a project"
          onClick={() => choose(null)}
        >
          <X size={12} />
        </button>
      ) : null}
      {creating ? (
        <CreateProjectPart
          part="CreateChatProjectDialog"
          creating={creating}
          onClose={() => setCreating(null)}
        />
      ) : null}
    </span>
  );
}
