import { Plus, Search } from "../ui/leanIcons";

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from "react";

import { useWorkbenchStore } from "../app/store";
import { ChatSearchPart, lazyParts } from "../features/chat/lazyParts";
import {
  CHAT_SEARCH_ALL_FOLDERS_ID,
  hasChatSearchFolderRequest,
  requestChatSearchInFolder,
  subscribeChatSearchFolderRequest,
} from "../features/chatSearch/chatSearchScope";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import { usesMetaModifier } from "../ui/shortcutLabel";
import {
  deriveAgentRailSections,
  groupAgentRailProjects,
} from "./agentChatRail";
import { AgentIconRail } from "./AgentIconRail";
import { useAgentPanelView } from "./agentPanelView";
import type * as ArchiveNoticeChunk from "./AgentRailArchiveNotice";
import type * as RailPanelsChunk from "./AgentRailPanels";
import {
  pruneAgentRailInteractions,
  useAgentRailInteractions,
} from "./agentRailInteractions";
import { AgentRailSections } from "./AgentRailSections";
import {
  expandAgentRailSection,
  useAgentRailViewPrefs,
} from "./agentRailViewPrefs";
import type * as CreateProjectChunk from "./CreateProjectDialog";
import { useRailChatId } from "./sideChatState";
const ArchiveNoticePart = lazyParts<typeof ArchiveNoticeChunk>(
  () => import("./AgentRailArchiveNotice"),
);
const CreateProjectPart = lazyParts<typeof CreateProjectChunk>(
  () => import("./CreateProjectDialog"),
);
const RailPanelPart = lazyParts<typeof RailPanelsChunk>(
  () => import("./AgentRailPanels"),
);

export function ChatSearchGate({
  triggerRef,
}: {
  triggerRef: RefObject<HTMLButtonElement | null>;
}) {
  const [armed, setArmed] = useState(hasChatSearchFolderRequest);
  useEffect(() => {
    if (armed) return;
    const arm = () => setArmed(true);
    if (hasChatSearchFolderRequest()) arm();
    return subscribeChatSearchFolderRequest(arm);
  }, [armed]);
  return armed ? (
    <ChatSearchPart
      part="ChatSearchControl"
      showTrigger={false}
      triggerRef={triggerRef}
    />
  ) : null;
}

export function LeftSidebar({
  inert = false,
  railRef,
}: {
  inert?: boolean;
  railRef?: (node: HTMLElement | null) => void;
} = {}) {
  const projects = useWorkbenchStore((state) => state.projects);
  const unscopedChats = useWorkbenchStore((state) => state.unscopedChats);
  const project = useWorkbenchStore((state) => state.project);
  const tabs = useWorkbenchStore((state) => state.tabs);
  const switchProject = useWorkbenchStore((state) => state.switchProject);
  const createChat = useWorkbenchStore((state) => state.openNewChatDraft);
  const interactions = useAgentRailInteractions();
  const view = useAgentPanelView((state) => state.view);
  const openSettings = useAgentSettingsState((state) => state.open);
  const loadRepositoryInventory = useWorkbenchStore(
    (state) => state.loadRepositoryInventory,
  );
  const projectGeneration = useWorkbenchStore(
    (state) => state.projectGeneration,
  );
  const nativeTransitionEpoch = useWorkbenchStore(
    (state) => state.nativeTransitionEpoch,
  );
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const railChatId = useRailChatId(activeTabId);
  const sidebarRef = useRef<HTMLElement>(null);
  const attachSidebar = useCallback(
    (node: HTMLElement | null) => {
      sidebarRef.current = node;
      railRef?.(node);
    },
    [railRef],
  );
  const previousChatId = useRef(railChatId);
  const [creatingProject, setCreatingProject] = useState(false);
  const searchTriggerRef = useRef<HTMLButtonElement>(null);
  const groups = useMemo(
    () => [
      ...groupAgentRailProjects({
        projects,
        currentProjectId: project?.isInternal ? null : (project?.id ?? null),
        currentChats: project?.isInternal
          ? []
          : tabs.filter((tab) => tab.kind === "chat"),
      }),
      ...groupAgentRailProjects({
        projects: unscopedChats,
        isInternal: true,
        currentProjectId: project?.isInternal ? project.id : null,
        currentChats: project?.isInternal
          ? tabs.filter((tab) => tab.kind === "chat")
          : [],
      }),
    ],
    [project, projects, unscopedChats, tabs],
  );
  const prefs = useAgentRailViewPrefs();
  useEffect(() => {
    if (projectTransitioning) return;
    if (previousChatId.current !== railChatId) {
      previousChatId.current = railChatId;
      const active = useWorkbenchStore
        .getState()
        .tabs.find((tab) => tab.id === railChatId);
      const sections = [
        "projects",
        active?.pinned ? "pinned" : `project:${project?.id}`,
      ];
      sections.forEach(expandAgentRailSection);
    }
  }, [railChatId, project?.id, projectTransitioning]);
  useEffect(() => {
    pruneAgentRailInteractions(
      new Set(groups.flatMap((group) => group.chats.map((chat) => chat.id))),
    );
  }, [groups]);
  const sections = useMemo(
    () =>
      deriveAgentRailSections(groups, undefined, prefs.ordering, railChatId),
    [groups, prefs.ordering, railChatId],
  );

  useEffect(() => {
    void loadRepositoryInventory();
  }, [
    loadRepositoryInventory,
    nativeTransitionEpoch,
    project?.id,
    projectGeneration,
    projectTransitioning,
  ]);

  const navigation =
    view === "archived" || view === "harnesses" ? (
      <div className="project-sidebar__scroll">
        <RailPanelPart
          part={
            view === "archived" ? "AgentArchivedPanel" : "AgentHarnessPanel"
          }
        />
      </div>
    ) : (
      <>
        {view === "home" ? (
          <div className="project-sidebar__actions">
            <button
              className="project-action project-action--primary"
              type="button"
              title="New chat"
              aria-label="New chat"
              aria-keyshortcuts={usesMetaModifier() ? "Meta+N" : "Control+N"}
              disabled={projectTransitioning}
              onClick={() => void createChat(null)}
            >
              <span className="project-action__box" aria-hidden="true">
                <Plus size={14} />
              </span>
              <span>New chat</span>
            </button>
            <button
              ref={searchTriggerRef}
              aria-label="Search chats"
              className="project-action"
              type="button"
              disabled={!project || projectTransitioning}
              onClick={() => {
                if (project)
                  requestChatSearchInFolder(CHAT_SEARCH_ALL_FOLDERS_ID);
              }}
            >
              <Search size={16} aria-hidden="true" />
              <span>Search</span>
            </button>
          </div>
        ) : null}
        <div className="project-sidebar__scroll">
          <AgentRailSections
            view={view}
            pinned={sections.pinned}
            projects={sections.projects}
            recent={sections.recent}
            unscoped={sections.unscoped}
            all={sections.all}
            allOverflowProjectId={sections.allOverflowProjectId}
            currentProjectId={project?.id ?? null}
            currentChatId={railChatId}
            prefs={prefs}
            createChat={createChat}
            onCreateProject={() => setCreatingProject(true)}
            switchProject={switchProject}
            onShowMore={requestChatSearchInFolder}
          />
          {interactions.undo ? (
            <ArchiveNoticePart part="AgentRailArchiveNotice" />
          ) : null}
        </div>
      </>
    );

  return (
    <aside
      className="project-sidebar"
      ref={attachSidebar}
      aria-label="Projects and chats"
      inert={inert}
      aria-hidden={inert || undefined}
    >
      <ChatSearchGate triggerRef={searchTriggerRef} />
      {creatingProject ? (
        <CreateProjectPart
          part="CreateProjectDialog"
          onClose={() => setCreatingProject(false)}
        />
      ) : null}
      <AgentIconRail inert={inert} onSettings={() => openSettings()} />
      {navigation}
    </aside>
  );
}
