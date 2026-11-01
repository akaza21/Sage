import {
  lazy,
  Suspense,
  useContext,
  useMemo,
  useRef,
  useLayoutEffect,
  type ReactNode,
  type RefObject,
} from "react";
import { FileText, Globe2, GitCompareArrows } from "../ui/leanIcons";
import { useStore } from "zustand";
import { acpProjectionStoreFor } from "../app/acpProjectionRegistry";
import {
  validFocusedSubagentPage,
  useSubagentPaneStore,
} from "../app/subagentPaneStore";
import { useWorkbenchStore } from "../app/store";
import { openAgentArtifact } from "../app/openAgentArtifact";
import { MenuPopover } from "../ui/MenuPopover";
import {
  liveAgent,
  projectTaskResources,
} from "./agentArtifactTabPresentation";
import { loadSubagentSurface } from "./loadSubagentSurface";
import { AgentToolsContext } from "./AgentToolsProvider";
import { useAgentChangesState } from "./agentChangesState";
import "./agent-resources.css";

const AgentTaskSubagentSummary = lazy(() =>
  loadSubagentSurface().then((module) => ({
    default: module.AgentTaskSubagentSummary,
  })),
);

export function AgentResourcesPopover({
  chatId,
  resourcesId,
  resourcesTriggerRef,
  showAllSources,
  onToggleSources,
  onDismiss,
}: {
  chatId: string;
  resourcesId: string;
  resourcesTriggerRef: RefObject<HTMLButtonElement | null>;
  showAllSources: boolean;
  onToggleSources: () => void;
  onDismiss: () => void;
}) {
  const project = useWorkbenchStore((state) => state.project);
  const generation = useWorkbenchStore((state) => state.projectGeneration);
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const page = useStore(acpProjectionStoreFor(chatId), (state) => state.page);
  const scope = useStore(acpProjectionStoreFor(chatId), (state) => state.scope);
  const validPage = validFocusedSubagentPage(
    page,
    scope,
    project?.id ?? null,
    chatId,
    generation,
    transitioning,
  );
  const sessionKey = `${page?.currentSessionId}:${page?.sessions.find((session) => session.id === page.currentSessionId)?.runtimeEpoch}`;
  const previousSession = useRef(sessionKey);
  const focusWithin = useRef(false);
  useLayoutEffect(() => {
    const trackFocus = () => {
      focusWithin.current = !!document
        .getElementById(resourcesId)
        ?.contains(document.activeElement);
    };
    trackFocus();
    document.addEventListener("focusin", trackFocus);
    return () => document.removeEventListener("focusin", trackFocus);
  }, [resourcesId]);
  useLayoutEffect(() => {
    if (previousSession.current === sessionKey) return;
    previousSession.current = sessionKey;
    if (focusWithin.current) resourcesTriggerRef.current?.focus();
    if (showAllSources) onToggleSources();
    onDismiss();
  }, [
    sessionKey,
    resourcesId,
    resourcesTriggerRef,
    showAllSources,
    onToggleSources,
    onDismiss,
  ]);
  const { launch } = useContext(AgentToolsContext);
  const { recentAgents, outputs, sources, changesCount } = useMemo(
    () => projectTaskResources(validPage ? page : null, chatId),
    [page, chatId, validPage],
  );
  const workingAgents = recentAgents.filter((agent) =>
    liveAgent(agent.status),
  ).length;
  const doneAgents = recentAgents.filter(
    (agent) => agent.status === "done",
  ).length;
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

  return (
    <MenuPopover
      id={resourcesId}
      className="agent-task-header__resource-panel"
      role="dialog"
      ariaLabel="Chat resources"
      open
      triggerRef={resourcesTriggerRef}
      onDismiss={() => onDismiss()}
    >
      <section aria-label="Changes">
        <button
          className="agent-task-header__resource-row"
          type="button"
          aria-label="Changes"
          disabled={!validPage || !page?.currentSessionId}
          title={
            !validPage || !page?.currentSessionId
              ? "No current Chat session"
              : "View this Chat's changes"
          }
          onClick={() => {
            launch("changes", chatId);
            onDismiss();
          }}
        >
          <GitCompareArrows size={14} aria-hidden="true" />
          <span>Changes</span>
          {changesCount > 0 ? (
            <span>
              {changesCount} recent {changesCount === 1 ? "file" : "files"}
            </span>
          ) : null}
        </button>
      </section>
      <section aria-label="Outputs">
        <strong>Outputs</strong>
        {outputs.length === 0 ? <p>Create a file or site</p> : null}
        {outputs.map((output) => (
          <button
            key={`${output.resource.rootId}:${output.label}`}
            className="agent-task-header__resource-row"
            type="button"
            aria-label={`Open output ${output.label}`}
            disabled={
              !validPage || output.owner.sessionId !== page?.currentSessionId
            }
            title={
              output.owner.sessionId !== page?.currentSessionId
                ? "This output belongs to an earlier session and cannot be opened here."
                : output.label
            }
            onClick={() => {
              void openAgentArtifact({
                owner: output.owner,
                target: output.target,
                select: true,
                resource: output.resource,
                ...(output.diffRef ? { diffRef: output.diffRef } : {}),
              }).then((opened) => {
                if (opened) {
                  useAgentChangesState.getState().select(false);
                  useSubagentPaneStore.getState().select(false);
                  onDismiss();
                }
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
              useAgentChangesState.getState().select(false);
              resourcesTriggerRef.current?.focus();
              onDismiss();
            }}
          />
        </Suspense>
      ) : null}
      <section aria-label="Background processes">
        <strong>Background processes</strong>
        <p>Process tracking is not exposed by this session.</p>
      </section>
      <section aria-label="Sources">
        <strong>Sources</strong>
        {sources.length === 0 ? <p>No sources reported.</p> : null}
        {(showAllSources ? sources : sources.slice(-3)).map((source, index) => (
          <span
            key={source.label + ":" + index}
            className="agent-task-header__resource-row"
            title={source.label + " - Recorded source; not a live browser link"}
          >
            <Globe2 size={14} aria-hidden="true" />
            <span>{source.label}</span>
          </span>
        ))}
      </section>
      {sources.length > 3 ? (
        <button
          className="agent-task-header__view-all"
          type="button"
          onClick={onToggleSources}
        >
          {showAllSources ? "Show less" : "View all"}
        </button>
      ) : null}
    </MenuPopover>
  );
}
