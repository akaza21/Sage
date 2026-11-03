import { ListChecks, Folder, Pencil } from "../ui/leanIcons";

import {
  useId,
  useLayoutEffect,
  useEffect,
  useRef,
  useState,
  useContext,
  useCallback,
} from "react";

import { useWorkbenchStore } from "../app/store";
import { acpProjectionStoreFor } from "../app/acpProjectionRegistry";
import { chatDisplayTitle } from "../domain/chatTitle";
import type { WorkbenchTab } from "../domain/workbench";
import type { ChatLiveSnapshot } from "../features/chat/acp/chatLiveSnapshot";
import type { AgentSessionChrome } from "../app/agentChatOwner";
import {
  AgentHeaderMore,
  ProjectChatActionMenu,
} from "./ProjectChatActionMenu";
import "./agent-subagent-summary.css";

import { AgentTaskStatus } from "./AgentTaskStatus";
import { isSideChatId } from "./sideChatState";
import { AgentTaskResourcesPart } from "./agentTaskHeaderLazy";
import { AgentToolsContext } from "./AgentToolsProvider";
import { AgentNavigationPart } from "./agentNavigationLazy";

function preloadResources() {
  void AgentTaskResourcesPart.preload().catch(() => undefined);
}

export function AgentTaskHeader({
  tab,
  chatCount,
  snapshot,
  sessionChrome,
  showNewChat = false,
}: {
  tab: WorkbenchTab | null;
  chatCount: number;
  snapshot: ChatLiveSnapshot | null;
  sessionChrome?: AgentSessionChrome | null;
  showNewChat?: boolean;
}) {
  const harnessName = useWorkbenchStore((state) => {
    const page = sessionChrome?.acpPage;
    const binding = page?.sessions.find(
      (session) => session.id === page.currentSessionId,
    )?.bindingId;
    const id =
      state.chatUiById[tab?.id ?? ""]?.harnessId ?? state.selectedHarnessId;
    return state.harnesses.find((harness) =>
      binding ? harness.bindingId === binding : harness.id === id,
    )?.name;
  });
  const project = useWorkbenchStore((state) => state.project);
  const { launch } = useContext(AgentToolsContext);
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const generation = useWorkbenchStore((state) => state.projectGeneration);
  const [menuOpen, setMenuOpen] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [resourcesOpen, setResourcesOpen] = useState(false);
  const [showAllSources, setShowAllSources] = useState(false);
  const resourcesSession = useRef("");
  useEffect(() => {
    const timer = setTimeout(preloadResources, 1000);
    return () => clearTimeout(timer);
  }, []);
  const resourcesTriggerRef = useRef<HTMLButtonElement | null>(null);
  const resourcesId = useId();
  const renameId = `${resourcesId}-rename`;
  const dismissPending = useCallback(() => {
    if (!document.getElementById(resourcesId)) setResourcesOpen(false);
    if (!document.getElementById(renameId)) setRenaming(false);
  }, [resourcesId, renameId]);
  useEffect(() => {
    if (!resourcesOpen && !renaming) return;
    const dismissOutside = (event: Event) => {
      if (
        event.target instanceof Node &&
        resourcesTriggerRef.current?.contains(event.target)
      )
        return;
      dismissPending();
    };
    document.addEventListener("pointerdown", dismissOutside, true);
    return () =>
      document.removeEventListener("pointerdown", dismissOutside, true);
  }, [resourcesOpen, renaming, dismissPending]);
  const resourcesScope = [
    project?.id,
    generation,
    tab?.id,
    projectTransitioning,
  ].join("\u0000");
  const [[previousScope, restoreFocus], setPreviousScope] = useState<
    [scope: string, restoreFocus: boolean]
  >([resourcesScope, false]);
  if (previousScope !== resourcesScope) {
    setPreviousScope([
      resourcesScope,
      resourcesOpen &&
        !!document
          .getElementById(resourcesId)
          ?.contains(document.activeElement),
    ]);
    setResourcesOpen(false);
    setShowAllSources(false);
    setRenaming(false);
    setMenuOpen(false);
  }
  useLayoutEffect(() => {
    if (restoreFocus) resourcesTriggerRef.current?.focus();
  }, [previousScope, restoreFocus]);
  if (!tab || !project) return null;
  const currentTab = tab;
  const currentProject = project;
  const title = chatDisplayTitle(currentTab.title);
  const heading = (
    <strong className="agent-task-header__title" title={title}>
      {title}
    </strong>
  );

  return (
    <header
      className="agent-task-header"
      onPointerEnter={preloadResources}
      onFocus={preloadResources}
      onKeyDownCapture={(event) => {
        if (!["Escape", "Tab"].includes(event.key)) return;
        dismissPending();
      }}
    >
      {showNewChat ? (
        <AgentNavigationPart part="AgentNewChatButton" />
      ) : (
        <button
          className="agent-task-header__pencil"
          type="button"
          aria-label="Rename chat"
          title="Rename chat"
          disabled={projectTransitioning || renaming}
          onClick={() => setRenaming(true)}
        >
          <Pencil size={15} aria-hidden="true" />
        </button>
      )}
      <div className="agent-task-header__breadcrumb" aria-label="Chat location">
        <Folder size={15} aria-hidden="true" />
        {renaming ? (
          <AgentTaskResourcesPart
            part="AgentTaskRenameForm"
            id={renameId}
            title={title}
            chatId={currentTab.id}
            projectId={currentProject.id}
            disabled={projectTransitioning}
            onCancel={() => setRenaming(false)}
            fallback={heading}
          />
        ) : (
          heading
        )}
      </div>
      <AgentTaskStatus
        snapshot={snapshot}
        harnessStatus={sessionChrome?.harnessStatus}
        harnessName={harnessName}
      />
      {isSideChatId(tab?.id) ? null : (
        <AgentHeaderMore
          scope={resourcesScope}
          title={title}
          projectName={currentProject.name}
          disabled={projectTransitioning || renaming}
          onChanges={() => launch("changes", currentTab.id)}
          onDismiss={() => setMenuOpen(false)}
        >
          <ProjectChatActionMenu
            chatId={currentTab.id}
            chatTitle={`${title} task`}
            pinned={currentTab.pinned}
            open={menuOpen}
            disabled={projectTransitioning || renaming}
            archiveDisabled={projectTransitioning || chatCount <= 1}
            projectId={currentProject.id}
            onOpenChange={setMenuOpen}
            onRename={() => setRenaming(true)}
            sessionChrome={sessionChrome}
            triggerLabel={`More chat actions in ${currentProject.name}`}
            triggerRole="menuitem"
          />
        </AgentHeaderMore>
      )}
      <div className="agent-task-header__resources">
        <button
          ref={resourcesTriggerRef}
          type="button"
          aria-label="Show chat resources"
          aria-controls={resourcesId}
          aria-haspopup="dialog"
          aria-expanded={resourcesOpen}
          title="Changes, Subagents, Background processes, and Sources"
          onClick={() => {
            const page = acpProjectionStoreFor(currentTab.id).getState().page;
            const session = page?.sessions.find(
              (entry) => entry.id === page.currentSessionId,
            );
            const scope = [
              resourcesScope,
              page?.currentSessionId,
              session?.runtimeEpoch,
            ].join("\u0000");
            if (resourcesSession.current !== scope) setShowAllSources(false);
            resourcesSession.current = scope;
            setResourcesOpen((open) => !open);
          }}
        >
          <ListChecks size={16} aria-hidden="true" />
        </button>
        {resourcesOpen ? (
          <AgentTaskResourcesPart
            part="AgentTaskResources"
            chatId={currentTab.id}
            resourcesId={resourcesId}
            resourcesTriggerRef={resourcesTriggerRef}
            showAllSources={showAllSources}
            onToggleSources={() => setShowAllSources((shown) => !shown)}
            onDismiss={() => setResourcesOpen(false)}
          />
        ) : null}
      </div>
    </header>
  );
}
