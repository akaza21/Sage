import { ListChecks, Folder, Pencil } from "../ui/leanIcons";

import {
  useId,
  useLayoutEffect,
  useRef,
  useState,
  useContext,
  lazy,
  Suspense,
  type FormEvent,
} from "react";

import { useWorkbenchStore } from "../app/store";
import { chatDisplayTitle } from "../domain/chatTitle";
import type { WorkbenchTab } from "../domain/workbench";
import type { ChatLiveSnapshot } from "../features/chat/acp/chatLiveSnapshot";
import type { AgentSessionChrome } from "../app/agentChatOwner";
import { agentChatMoveDestinations } from "./agentChatRail";
import {
  AgentHeaderMore,
  ProjectChatActionMenu,
} from "./ProjectChatActionMenu";
import "./agent-subagent-summary.css";

import { AgentTaskRenameForm, AgentTaskStatus } from "./AgentTaskStatus";
import { isSideChatId } from "./sideChatState";
import { AgentToolsContext } from "./AgentToolsProvider";

const AgentResourcesPopover = lazy(() =>
  import("./AgentResourcesPopover").then((module) => ({
    default: module.AgentResourcesPopover,
  })),
);

export function AgentTaskHeader({
  tab,
  chatCount,
  snapshot,
  sessionChrome,
}: {
  tab: WorkbenchTab | null;
  chatCount: number;
  snapshot: ChatLiveSnapshot | null;
  sessionChrome?: AgentSessionChrome | null;
}) {
  const project = useWorkbenchStore((state) => state.project);
  const { launch } = useContext(AgentToolsContext);
  const projects = useWorkbenchStore((state) => state.projects);
  const renameChat = useWorkbenchStore((state) => state.renameChat);
  const archiveChat = useWorkbenchStore((state) => state.archiveChat);
  const pinChat = useWorkbenchStore((state) => state.pinChat);
  const moveChat = useWorkbenchStore((state) => state.moveChat);
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const generation = useWorkbenchStore((state) => state.projectGeneration);
  const [menuOpen, setMenuOpen] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [draft, setDraft] = useState("");
  const [renameError, setRenameError] = useState<string | null>(null);
  const [resourcesOpen, setResourcesOpen] = useState(false);
  const resourcesTriggerRef = useRef<HTMLButtonElement | null>(null);
  const resourcesId = useId();
  const resourcesScope = [
    project?.id,
    generation,
    tab?.id,
    projectTransitioning,
  ].join("\u0000");
  const [previousScope, setPreviousScope] = useState({
    key: resourcesScope,
    restoreFocus: false,
  });
  if (previousScope.key !== resourcesScope) {
    setPreviousScope({
      key: resourcesScope,
      restoreFocus:
        resourcesOpen &&
        !!document
          .getElementById(resourcesId)
          ?.contains(document.activeElement),
    });
    setResourcesOpen(false);
    setMenuOpen(false);
  }
  useLayoutEffect(() => {
    if (previousScope.restoreFocus) resourcesTriggerRef.current?.focus();
  }, [previousScope]);

  if (!tab || !project) return null;
  const currentTab = tab;
  const currentProject = project;
  const title = chatDisplayTitle(currentTab.title);
  const destinations = agentChatMoveDestinations(projects, currentProject.id);

  function beginRename() {
    setDraft(title);
    setRenameError(null);
    setRenaming(true);
  }

  async function submitRename(event: FormEvent) {
    event.preventDefault();
    setRenameError(null);
    const committed = await renameChat(currentTab.id, draft, currentProject.id);
    if (committed) {
      setRenaming(false);
      return;
    }
    setRenameError(
      useWorkbenchStore.getState().error ?? "Chat rename could not be saved",
    );
  }

  return (
    <header className="agent-task-header">
      <button
        className="agent-task-header__pencil"
        type="button"
        aria-label="Rename chat"
        title="Rename chat"
        disabled={projectTransitioning || renaming}
        onClick={beginRename}
      >
        <Pencil size={15} aria-hidden="true" />
      </button>
      <div className="agent-task-header__breadcrumb" aria-label="Chat location">
        <Folder size={15} aria-hidden="true" />
        {renaming ? (
          <AgentTaskRenameForm
            value={draft}
            error={renameError}
            disabled={projectTransitioning}
            onChange={setDraft}
            onSubmit={submitRename}
            onCancel={() => {
              setRenaming(false);
              setRenameError(null);
            }}
          />
        ) : (
          <strong className="agent-task-header__title" title={title}>
            {title}
          </strong>
        )}
      </div>
      <AgentTaskStatus
        snapshot={snapshot}
        harnessStatus={sessionChrome?.harnessStatus}
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
            chatTitle={`${title} task`}
            pinned={currentTab.pinned}
            open={menuOpen}
            disabled={projectTransitioning || renaming}
            archiveDisabled={projectTransitioning || chatCount <= 1}
            destinations={destinations}
            onOpenChange={setMenuOpen}
            onPin={() => void pinChat(currentTab.id, currentProject.id)}
            onRename={beginRename}
            onMove={(targetProjectId) =>
              void moveChat(currentTab.id, targetProjectId, currentProject.id)
            }
            onArchive={() => void archiveChat(currentTab.id, currentProject.id)}
            triggerRef={() => undefined}
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
          onClick={() => setResourcesOpen((open) => !open)}
        >
          <ListChecks size={16} aria-hidden="true" />
        </button>
        {resourcesOpen ? (
          <Suspense fallback={<p role="status">Loading resources...</p>}>
            <AgentResourcesPopover
              chatId={tab.id}
              resourcesId={resourcesId}
              resourcesTriggerRef={resourcesTriggerRef}
              onDismiss={() => setResourcesOpen(false)}
            />
          </Suspense>
        ) : null}
      </div>
    </header>
  );
}
