import {
  NEW_PROJECT_ACTION,
  NO_PROJECT_ACTION,
} from "../../ui/projectPickerActions";
import { useMemo, useState } from "react";
import { projectNameOption } from "../../domain/projectNameOption";
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
type ProjectScopeVariant = "inline" | "caption" | "menuItem";
const CLEAR_LABEL = "Remove project from this chat";

export function ProjectScopeChip({
  projectName,
  disabled = false,
  variant = "inline",
  ...pickerProps
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
  const readOnly =
    disabled ||
    switchBlocked ||
    (draft
      ? draft.submission.status === "submitting" ||
        draft.submission.status === "unconfirmed"
      : !!sessionId);
  const candidates = useMemo(() => {
    return [
      ...projects.map((project) => ({
        ...projectNameOption(project, projects),
        leadingIcon: Folder,
      })),
      NEW_PROJECT_ACTION,
      ...(scoped ? [NO_PROJECT_ACTION] : []),
    ];
  }, [projects, scoped]);
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
      className={`project-scope project-scope--${menuItem ? "menu-item" : variant}`}
    >
      <SearchablePicker
        {...pickerProps}
        triggerRole={menuItem ? "menuitem" : undefined}
        className="project-scope__picker"
        appearance="plain"
        menuAppearance
        searchPlaceholder="Search projects"
        triggerClassName={
          menuItem ? "menu-item composer__add-action" : undefined
        }
        chevron={false}
        portalTarget={menuItem ? "closest-popover" : "closest-dialog"}
        popoverClassName="composer__popover"
        ariaLabel={scoped && !menuItem ? `Project: ${label}` : triggerLabel}
        stateTitle={
          readOnly ? `Project: ${label}` : "Change the project for this chat"
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
            <span className="menu-description">
              Choose project for this chat
            </span>
          ) : undefined
        }
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
          aria-label={CLEAR_LABEL}
          title={CLEAR_LABEL}
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
