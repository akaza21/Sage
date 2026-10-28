import { FileText, Globe2, ListChecks, Folder, Pencil } from "../ui/leanIcons";

import {
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useContext,
  lazy,
  Suspense,
  type FormEvent,
  type ReactNode,
} from "react";
import { useStore } from "zustand";

import { acpProjectionStoreFor } from "../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../app/store";
import { openAgentArtifact } from "../app/openAgentArtifact";
import { useAgentArtifactFollowStore } from "../app/agentArtifactFollowStore";
import { chatDisplayTitle } from "../domain/chatTitle";
import type { WorkbenchTab } from "../domain/workbench";
import type { ChatLiveSnapshot } from "../features/chat/acp/chatLiveSnapshot";
import type { AgentSessionChrome } from "../app/agentChatOwner";
import { agentChatMoveDestinations } from "./agentChatRail";
import {
  AgentHeaderMore,
  ProjectChatActionMenu,
} from "./ProjectChatActionMenu";
import { MenuPopover } from "../ui/MenuPopover";
import {
  liveAgent,
  projectTaskResources,
} from "./agentArtifactTabPresentation";
import { validFocusedSubagentPage } from "../app/subagentPaneStore";
import "./agent-subagent-summary.css";

import { AgentTaskRenameForm, AgentTaskStatus } from "./AgentTaskStatus";
import { isSideChatId } from "./sideChatState";
import { loadSubagentSurface } from "./loadSubagentSurface";
import { AgentToolsContext } from "./AgentToolsProvider";

// The Subagents region and its rows load with the lazy SubagentSurface chunk;
// the header renders the counts synchronously and hands them to it.
const AgentTaskSubagentSummary = lazy(() =>
  loadSubagentSurface().then((module) => ({
    default: module.AgentTaskSubagentSummary,
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
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activateTab = useWorkbenchStore((state) => state.activateTab);
  const [menuOpen, setMenuOpen] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [draft, setDraft] = useState("");
  const [renameError, setRenameError] = useState<string | null>(null);
  const [resourcesOpen, setResourcesOpen] = useState(false);
  const [showAllSources, setShowAllSources] = useState(false);
  const resourcesTriggerRef = useRef<HTMLButtonElement | null>(null);
  const resourcesId = useId();
  const page = useStore(
    acpProjectionStoreFor(tab?.id ?? null),
    (state) => state.page,
  );
  const scope = useStore(
    acpProjectionStoreFor(tab?.id ?? null),
    (state) => state.scope,
  );
  const resourcesScope = [
    project?.id,
    generation,
    tab?.id,
    page?.currentSessionId,
    page?.sessions.find((session) => session.id === page.currentSessionId)
      ?.runtimeEpoch,
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
    setShowAllSources(false);
    setMenuOpen(false);
  }
  useLayoutEffect(() => {
    if (previousScope.restoreFocus) resourcesTriggerRef.current?.focus();
  }, [previousScope]);
  const validPage = validFocusedSubagentPage(
    page,
    scope,
    project?.id ?? null,
    tab?.id ?? null,
    generation,
    projectTransitioning,
  );
  const { recentAgents, outputs, sources } = useMemo(
    () =>
      projectTaskResources(
        resourcesOpen && validPage ? page : null,
        tab?.id ?? null,
      ),
    [page, resourcesOpen, tab?.id, validPage],
  );
  // Each retained spawn invocation is one subagent, even when a child thread
  // id is reused; repeated observations of one spawn share its entry.
  const workingAgents = recentAgents.filter((agent) =>
    liveAgent(agent.status),
  ).length;
  const doneAgents = recentAgents.filter(
    (agent) => agent.status === "done",
  ).length;
  // The decorative glyphs arrive with the lazy region.
  const agentCounts = (glyphs?: ReactNode) => (
    <div className="agent-task-header__agent-summary">
      <span
        className="agent-task-header__agent-metric"
        aria-label={`Recent subagents working: ${workingAgents}`}
      >
        {glyphs}
        {workingAgents} working
      </span>
      <span
        className="agent-task-header__agent-metric"
        aria-label={`Recent subagents done: ${doneAgents}`}
      >
        {doneAgents} done
      </span>
    </div>
  );

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
          title="Outputs, Subagents, and Sources"
          onClick={() => setResourcesOpen((open) => !open)}
        >
          <ListChecks size={16} aria-hidden="true" />
        </button>
        {resourcesOpen ? (
          <MenuPopover
            id={resourcesId}
            className="agent-task-header__resource-panel"
            role="dialog"
            ariaLabel="Chat resources"
            open
            triggerRef={resourcesTriggerRef}
            onDismiss={() => setResourcesOpen(false)}
          >
            <section aria-label="Outputs">
              <strong>Outputs</strong>
              {outputs.length === 0 ? <p>Create a file or site</p> : null}
              {outputs.map((output) => (
                <button
                  key={`${output.resource.rootId}:${output.label}`}
                  className="agent-task-header__resource-row"
                  type="button"
                  aria-label={`Open output ${output.label}`}
                  title={output.label}
                  onClick={() => {
                    void openAgentArtifact({
                      owner: output.owner,
                      target: output.target,
                      resource: output.resource,
                      ...(output.diffRef ? { diffRef: output.diffRef } : {}),
                    }).then((opened) => {
                      if (opened) setResourcesOpen(false);
                    });
                  }}
                >
                  <FileText size={14} aria-hidden="true" />
                  <span>{output.label}</span>
                </button>
              ))}
            </section>
            {recentAgents.length > 0 ? (
              <Suspense fallback={agentCounts()}>
                <AgentTaskSubagentSummary
                  recentAgents={recentAgents}
                  working={workingAgents}
                  counts={agentCounts}
                  onSelect={() => {
                    resourcesTriggerRef.current?.focus();
                    setResourcesOpen(false);
                  }}
                />
              </Suspense>
            ) : null}
            <section aria-label="Sources">
              <strong>Sources</strong>
              {(showAllSources ? sources : sources.slice(-3)).map(
                (source, index) => {
                  const browserTab = tabs.find(
                    (candidate) =>
                      candidate.id === source.tabId &&
                      candidate.kind === "browser",
                  );
                  const content = (
                    <>
                      <Globe2 size={14} aria-hidden="true" />
                      <span>{source.label}</span>
                    </>
                  );
                  return browserTab &&
                    page?.currentSessionId &&
                    page.projectId === project.id ? (
                    <button
                      key={`${source.label}:${source.tabId}:${index}`}
                      className="agent-task-header__resource-row"
                      type="button"
                      aria-label={`Open source ${source.label}`}
                      title={browserTab.title}
                      onClick={() => {
                        useAgentArtifactFollowStore.getState().pin(
                          {
                            projectId: page.projectId,
                            chatId: page.chatId,
                            sessionId: page.currentSessionId!,
                          },
                          { kind: "tab", tabId: browserTab.id },
                        );
                        void activateTab(browserTab.id);
                        setResourcesOpen(false);
                      }}
                    >
                      {content}
                    </button>
                  ) : (
                    <span
                      key={`${source.label}:${index}`}
                      className="agent-task-header__resource-row"
                      title={source.label}
                    >
                      {content}
                    </span>
                  );
                },
              )}
            </section>
            {sources.length > 3 ? (
              <button
                className="agent-task-header__view-all"
                type="button"
                onClick={() => setShowAllSources((shown) => !shown)}
              >
                {showAllSources ? "Show less" : "View all"}
              </button>
            ) : null}
          </MenuPopover>
        ) : null}
      </div>
    </header>
  );
}
