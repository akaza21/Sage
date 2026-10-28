import {
  lazy,
  Suspense,
  useMemo,
  useEffect,
  useRef,
  type FormEvent,
  useState,
  type ReactNode,
  type RefObject,
} from "react";
import { useStore } from "zustand";
import type { AcpChatPage } from "../domain/acp";
import { useWorkbenchStore } from "../app/store";
import { acpProjectionStoreFor } from "../app/acpProjectionRegistry";
import { validFocusedSubagentPage } from "../app/subagentPaneStore";
import { openAgentArtifact } from "../app/openAgentArtifact";
import { useAgentArtifactFollowStore } from "../app/agentArtifactFollowStore";
import { FileText, Globe2, Check, X } from "../ui/leanIcons";
import { MAX_CHAT_TITLE_BYTES } from "../domain/chatTitle";
import { MenuPopover } from "../ui/MenuPopover";
import {
  liveAgent,
  projectTaskResources,
} from "./agentArtifactTabPresentation";
import { loadSubagentSurface } from "./loadSubagentSurface";

export function AgentTaskRenameForm({
  id,
  title,
  chatId,
  projectId,
  disabled,
  onCancel,
}: {
  id: string;
  title: string;
  chatId: string;
  projectId: string;
  disabled: boolean;
  onCancel: () => void;
}) {
  const renameChat = useWorkbenchStore((state) => state.renameChat);
  const [value, setValue] = useState(title);
  const [error, setError] = useState<string | null>(null);
  async function onSubmit(event: FormEvent) {
    event.preventDefault();
    setError(null);
    if (await renameChat(chatId, value, projectId)) onCancel();
    else
      setError(
        useWorkbenchStore.getState().error ?? "Chat rename could not be saved",
      );
  }
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    input.current?.focus();
  }, []);
  return (
    <form id={id} className="agent-task-header__rename" onSubmit={onSubmit}>
      <input
        ref={input}
        aria-label="New chat name"
        aria-invalid={error ? true : undefined}
        maxLength={MAX_CHAT_TITLE_BYTES}
        value={value}
        disabled={disabled}
        onChange={(event) => setValue(event.currentTarget.value)}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            onCancel();
          }
        }}
      />
      <button type="submit" aria-label="Save chat name" disabled={disabled}>
        <Check size={14} aria-hidden="true" />
      </button>
      <button
        type="button"
        aria-label="Cancel chat rename"
        disabled={disabled}
        onClick={onCancel}
      >
        <X size={14} aria-hidden="true" />
      </button>
      {error ? <span role="alert">{error}</span> : null}
    </form>
  );
}

const AgentTaskSubagentSummary = lazy(() =>
  loadSubagentSurface().then((module) => ({
    default: module.AgentTaskSubagentSummary,
  })),
);

export function AgentTaskResources({
  page,
  chatId,
  projectId,
  id,
  triggerRef,
  showAllSources,
  onToggleSources,
  onClose,
}: {
  page: AcpChatPage | null;
  chatId: string;
  projectId: string;
  id: string;
  triggerRef: RefObject<HTMLButtonElement | null>;
  showAllSources: boolean;
  onToggleSources: () => void;
  onClose: () => void;
}) {
  const activateTab = useWorkbenchStore((state) => state.activateTab);
  const tabs = useWorkbenchStore((state) => state.tabs);
  const generation = useWorkbenchStore((state) => state.projectGeneration);
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const scope = useStore(acpProjectionStoreFor(chatId), (state) => state.scope);
  const validPage = validFocusedSubagentPage(
    page,
    scope,
    projectId,
    chatId,
    generation,
    transitioning,
  );
  const { recentAgents, outputs, sources } = useMemo(
    () => projectTaskResources(validPage ? page : null, chatId),
    [page, chatId, validPage],
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

  return (
    <MenuPopover
      id={id}
      className="agent-task-header__resource-panel"
      role="dialog"
      ariaLabel="Chat resources"
      open
      triggerRef={triggerRef}
      onDismiss={() => onClose()}
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
                if (opened) onClose();
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
              triggerRef.current?.focus();
              onClose();
            }}
          />
        </Suspense>
      ) : null}
      <section aria-label="Sources">
        <strong>Sources</strong>
        {(showAllSources ? sources : sources.slice(-3)).map((source, index) => {
          const browserTab = tabs.find(
            (candidate) =>
              candidate.id === source.tabId && candidate.kind === "browser",
          );
          const content = (
            <>
              <Globe2 size={14} aria-hidden="true" />
              <span>{source.label}</span>
            </>
          );
          return browserTab &&
            page?.currentSessionId &&
            page.projectId === projectId ? (
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
                onClose();
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
        })}
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
