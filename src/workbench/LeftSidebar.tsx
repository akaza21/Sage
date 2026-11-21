import { Search, Settings, SquarePen } from "../ui/leanIcons";

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from "react";

import { unsentChatIdsJson } from "../app/unsentChatIds";
import { usePinnedAgentChatOwner } from "../app/agentChatOwner";
import { useWorkbenchStore } from "../app/store";
import { harnessHasSignInCheck } from "../domain/harness";
import {
  ChatSearchPart,
  ComposerOptionsPart,
  lazyParts,
} from "../features/chat/lazyParts";
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
import type * as ArchiveNoticeChunk from "./AgentRailArchiveNotice";
import { AgentPrimaryNavigation } from "./AgentRailOptionsMenu";
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
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const project = useWorkbenchStore((state) => state.project);
  const namedProject = project?.isInternal ? null : project;
  const avatar = (namedProject?.name.trim()[0] ?? "L").toLocaleUpperCase();
  const tabs = useWorkbenchStore((state) => state.tabs);
  const unsentChatIds = useWorkbenchStore(unsentChatIdsJson);
  const switchProject = useWorkbenchStore((state) => state.switchProject);
  const createChat = useWorkbenchStore((state) => state.openNewChatDraft);
  const interactions = useAgentRailInteractions();
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
  const viewedChatId = useRailChatId(usePinnedAgentChatOwner() ?? activeTabId);
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
  // Native owns discovery. These bounded local exceptions keep the currently
  // open chat and input the user has not sent yet reachable until released.
  const groups = useMemo(() => {
    const protectedChatIds: string[] = JSON.parse(unsentChatIds);
    const currentChats = tabs.filter(
      (tab) =>
        tab.kind === "chat" &&
        (tab.chatListed !== false ||
          tab.id === viewedChatId ||
          protectedChatIds.includes(tab.id)),
    );
    return [false, true].flatMap((isInternal) => {
      const active = Boolean(project?.isInternal) === isInternal;
      return groupAgentRailProjects({
        projects: isInternal ? unscopedChats : projects,
        isInternal,
        currentProjectId: active ? (project?.id ?? null) : null,
        currentChats: active ? currentChats : [],
      });
    });
  }, [project, projects, unscopedChats, tabs, viewedChatId, unsentChatIds]);
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
    () => deriveAgentRailSections(groups, prefs.ordering, railChatId),
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

  const navigation = (
    <>
      <div className="project-sidebar__actions">
        <div className="project-sidebar__brand">
          <strong>Sage</strong>
          <button
            ref={searchTriggerRef}
            type="button"
            aria-label="Search chats"
            title="Search chats"
            disabled={!project || projectTransitioning}
            onClick={() => {
              if (project)
                requestChatSearchInFolder(CHAT_SEARCH_ALL_FOLDERS_ID);
            }}
          >
            <Search size={15} aria-hidden="true" />
          </button>
        </div>
        <button
          className="project-action project-action--primary"
          type="button"
          title="New chat"
          aria-label="New chat"
          aria-keyshortcuts={usesMetaModifier() ? "Meta+N" : "Control+N"}
          disabled={projectTransitioning}
          onClick={() => void createChat()}
        >
          <SquarePen size={16} aria-hidden="true" />
          <span>New chat</span>
        </button>
      </div>

      <div className="project-sidebar__scroll">
        <AgentRailSections
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

      <footer className="project-sidebar__account">
        {!inert && harnesses.some(harnessHasSignInCheck) ? (
          <ComposerOptionsPart part="HarnessAccountMenu" avatar={avatar} />
        ) : (
          <span className="account-avatar" aria-hidden="true">
            {avatar}
          </span>
        )}
        <span className="agent-rail-identity">
          <strong>{namedProject?.name ?? "Local"}</strong>
          <span>Local workspace</span>
        </span>
        <button
          className="project-sidebar__settings"
          type="button"
          title="Open settings"
          aria-label="Open settings"
          onClick={() => openSettings()}
        >
          <Settings size={16} aria-hidden="true" />
        </button>
      </footer>
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
      <AgentPrimaryNavigation />
      {navigation}
    </aside>
  );
}
