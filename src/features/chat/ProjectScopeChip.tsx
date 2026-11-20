import { useMemo, useState } from "react";
import { currentProjectLayout } from "../../app/projectProjection";
import { usePinnedAgentChatOwner } from "../../app/agentChatOwner";
import { hasActiveAcpTurn } from "../../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../app/store";
import { selectProjectSwitchBlocked } from "../../app/workbenchSelectors";
import { Folder, X } from "../../ui/leanIcons";
import { SearchablePicker } from "../../ui/SearchablePicker";
import type { MenuPlacement } from "../../ui/menuPopoverPosition";
import { lazyParts } from "./lazyParts";
import type * as CreateProjectChunk from "./CreateChatProjectDialog";
import type { ChatProjectCreation } from "./CreateChatProjectDialog";
import "./project-scope.css";

const CreateProjectPart = lazyParts<typeof CreateProjectChunk>(
  () => import("./CreateChatProjectDialog"),
);
const NONE = "scope:none";
const NEW = "scope:new";
type ProjectScopeVariant = "inline" | "caption" | "menuItem";

function shortenedParent(path: string) {
  return path.length > 64 ? `...${path.slice(-61)}` : path;
}

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
  const scoped = draft
    ? !!projectId
    : !project?.isInternal && projectName !== "Choose project";
  const label = scoped
    ? draft
      ? (projects.find((project) => project.id === projectId)?.name ??
        projectName)
      : projectName
    : "Choose project";
  const readOnly =
    disabled ||
    switchBlocked ||
    (draft
      ? draft.submission.status === "submitting" ||
        draft.submission.status === "unconfirmed"
      : !!sessionId);
  const candidates = useMemo(() => {
    const counts = new Map<string, number>();
    projects.forEach((project) =>
      counts.set(project.name, (counts.get(project.name) ?? 0) + 1),
    );
    return [
      ...projects.map((project) => {
        const duplicate = (counts.get(project.name) ?? 0) > 1;
        const parent = project.parentPath;
        return {
          id: project.id,
          label: project.id === projectId ? label : project.name,
          subtitle: duplicate
            ? parent
              ? shortenedParent(parent)
              : "No source folder"
            : undefined,
          accessibleLabel: duplicate
            ? `${project.name}, ${parent ?? "No source folder"}${projects.some((other) => other.id !== project.id && other.name === project.name && other.parentPath === parent) ? `, ${project.id}` : ""}`
            : project.name,
        };
      }),
      { id: NEW, label: "New project", alwaysVisible: true },
      { id: NONE, label: "Don't work in a project", alwaysVisible: true },
    ];
  }, [projects, projectId, label]);
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
        triggerClassName={
          menuItem ? "menu-item composer__add-action" : undefined
        }
        chevron={false}
        portalTarget={menuItem ? "closest-popover" : "closest-dialog"}
        popoverClassName="composer__popover"
        popoverPlacement={popoverPlacement}
        ariaLabel={
          menuItem
            ? "Work in a project"
            : scoped
              ? `Project: ${label}`
              : "Choose project"
        }
        stateTitle={
          readOnly ? `Project: ${label}` : "Change the project for this chat"
        }
        triggerIcon={
          caption ? undefined : (
            <Folder
              className="project-scope__icon"
              size={14}
              aria-hidden="true"
            />
          )
        }
        triggerLabel={
          menuItem ? (
            "Work in a project"
          ) : (
            <span className={caption ? "project-scope__caption" : undefined}>
              {label}
            </span>
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
        emptyLabel={label}
        selectedId={projectId}
        onSelect={(id) => {
          if (id !== NEW) {
            choose(id === NONE ? null : id);
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
          <X size={12} aria-hidden="true" />
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
