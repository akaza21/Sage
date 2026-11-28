import {
  lazy,
  Suspense,
  useContext,
  useMemo,
  useState,
  type FormEvent,
  type ReactNode,
  type RefObject,
} from "react";
import { useStore } from "zustand";
import {
  ArrowUp,
  FileDiff,
  Globe2,
  Search,
  TerminalSquare,
} from "../ui/leanIcons";
import { Folders } from "../ui/leanIconNodesLazy";
import { acpProjectionStoreFor } from "../app/acpProjectionRegistry";
import { validFocusedSubagentPage } from "../app/subagentPaneStore";
import { useWorkbenchStore } from "../app/store";
import { resolveBrowserAddress } from "../features/browser/browserAddress";
import { NoticePart } from "../features/chat/lazyParts";
import { decodeSessionSurface } from "../services/acpSessionSurface";
import {
  liveAgent,
  projectRecentAgents,
  type RecentAgent,
} from "./agentArtifactTabPresentation";
import {
  openPaneBrowser,
  useBrowserPaneDisabled,
} from "./AgentArtifactPaneControls";
import { useAgentChangesState } from "./agentChangesState";
import { selectAgentPaneView } from "./agentPaneViews";
import { AgentToolsContext } from "./AgentToolsProvider";
import { loadSubagentSurface } from "./loadSubagentSurface";
import { projectTaskResources } from "./taskResourceProjection";

const AgentTaskSubagentSummary = lazy(() =>
  loadSubagentSurface().then((module) => ({
    default: module.AgentTaskSubagentSummary,
  })),
);

export function useOverviewPage(chatId: string) {
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
  return { page, validPage };
}

const countBadge = (count: number) => (
  <span className="agent-overview__badge">{count}</span>
);

/**
 * The entries the Overview card and the pane's launcher share: this Chat's
 * Changes, Files, Browser and Terminal, then its subagents and background
 * tasks with counts. A tool the runtime cannot open is left out.
 */
export function AgentOverviewEntries({
  chatId,
  onDone,
  focusReturn,
  children,
}: {
  chatId: string;
  onDone: () => void;
  focusReturn?: RefObject<HTMLButtonElement | null>;
  children?: ReactNode;
}) {
  const { page, validPage } = useOverviewPage(chatId);
  const { launch, terminalAvailable } = useContext(AgentToolsContext);
  const changesCount = useMemo(
    () => projectTaskResources(validPage ? page : null, chatId).changesCount,
    [page, chatId, validPage],
  );
  const recentAgents = useMemo(
    () => projectRecentAgents(validPage ? page : null, chatId),
    [page, chatId, validPage],
  );
  // Counts what BackgroundProcesses lists: background tasks, plus running
  // subagents that can still be stopped.
  const taskCount =
    decodeSessionSurface(validPage ? page?.sessionSurface : null)?.activities
      .filter(
        ({ kind, state, stoppable }) =>
          kind === "backgroundTask" ||
          (kind === "subagent" && state === "running" && stoppable),
      ).length ?? 0;
  const noSession = !validPage || !page?.currentSessionId;
  const browserDisabled = useBrowserPaneDisabled();
  const entry = (
    label: string,
    icon: ReactNode,
    run: () => void,
    detail?: ReactNode,
    disabled = false,
  ) => (
    <button
      className="agent-overview__row"
      type="button"
      aria-label={label}
      disabled={disabled}
      onClick={() => {
        useAgentChangesState.getState().select(false);
        run();
        onDone();
      }}
    >
      {icon}
      <span>{label}</span>
      {detail}
    </button>
  );
  const agentCounts = (agents: readonly RecentAgent[], glyphs?: ReactNode) => {
    const working = agents.filter((agent) => liveAgent(agent.status)).length;
    const done = agents.filter((agent) => agent.status === "done").length;
    return (
      <div className="agent-task-header__agent-summary">
        <span
          className="agent-task-header__agent-metric"
          aria-label={`Recent subagents working: ${working}`}
        >
          {glyphs}
          {working} working
        </span>
        <span
          className="agent-task-header__agent-metric"
          aria-label={`Recent subagents done: ${done}`}
        >
          {done} done
        </span>
      </div>
    );
  };
  return (
    <>
      <section aria-label="Open" className="agent-overview__entries">
        {entry(
          "Changes",
          <FileDiff size={16} aria-hidden="true" />,
          () => launch("changes", chatId),
          changesCount > 0 ? countBadge(changesCount) : null,
          noSession,
        )}
        {entry(
          "Open file",
          <Folders size={16} aria-hidden="true" />,
          () => launch("files", chatId),
          null,
          noSession,
        )}
        {entry(
          "Browser",
          <Globe2 size={16} aria-hidden="true" />,
          () => openPaneBrowser(),
          null,
          browserDisabled,
        )}
        {terminalAvailable
          ? entry(
              "Terminal",
              <TerminalSquare size={16} aria-hidden="true" />,
              () => launch("terminal", chatId),
            )
          : null}
      </section>
      {children}
      {recentAgents.length > 0 ? (
        <Suspense fallback={agentCounts(recentAgents)}>
          <AgentTaskSubagentSummary
            recentAgents={recentAgents}
            counts={agentCounts}
            onSelect={() => {
              useAgentChangesState.getState().select(false);
              focusReturn?.current?.focus();
              onDone();
            }}
          />
        </Suspense>
      ) : null}
      {taskCount > 0 && validPage && page?.sessionSurface ? (
        <section aria-label="Background tasks">
          <strong className="agent-overview__heading">
            Background tasks{countBadge(taskCount)}
          </strong>
          {/* Stop shares the transcript's session controls, loaded with them. */}
          <NoticePart
            part="ConversationView"
            view="BackgroundProcesses"
            surface={page.sessionSurface}
            chatId={chatId}
          />
        </section>
      ) : null}
    </>
  );
}

/** The pane's "+" page: a search-or-URL field above the Overview entries. */
export function AgentPaneLauncher({ chatId }: { chatId: string | null }) {
  const [error, setError] = useState<string | null>(null);
  const done = () => selectAgentPaneView(null);
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const field = event.currentTarget.elements.namedItem("address");
    const text = field instanceof HTMLInputElement ? field.value.trim() : "";
    if (!text) return;
    try {
      openPaneBrowser(resolveBrowserAddress(text));
      done();
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
    }
  };
  return (
    <section className="agent-pane-launcher" aria-label="New tab">
      <form className="agent-pane-launcher__address" onSubmit={submit}>
        <Search size={16} aria-hidden="true" />
        <input
          name="address"
          aria-label="Search or enter a URL"
          aria-invalid={error ? true : undefined}
          placeholder="Search or enter a URL"
          autoComplete="off"
          spellCheck={false}
          onChange={() => setError(null)}
        />
      </form>
      {error ? <p role="alert">{error}</p> : null}
      {chatId ? <AgentOverviewEntries chatId={chatId} onDone={done} /> : null}
    </section>
  );
}

const nextFrame = () =>
  new Promise((resolve) => requestAnimationFrame(() => resolve(undefined)));

/**
 * While the pane covers the Chat, this pill sends into it through the
 * Chat's own composer, so harness, model, queueing and approvals apply.
 */
export function AgentPaneAskPill({ chatId }: { chatId: string }) {
  const [text, setText] = useState("");
  const send = async (event: FormEvent) => {
    event.preventDefault();
    const prompt = text.trim();
    const state = useWorkbenchStore.getState();
    const intent = prompt ? state.captureChatOwnership(chatId) : null;
    if (!intent || !state.setChatComposer(intent, prompt)) return;
    setText("");
    // The composer submits what it rendered; wait for the new text to land.
    await nextFrame();
    await nextFrame();
    document
      .querySelector<HTMLFormElement>(
        `.agent-chat-pane[data-chat-id="${CSS.escape(chatId)}"] form.composer`,
      )
      ?.requestSubmit();
  };
  return (
    <form className="agent-pane-ask" onSubmit={(event) => void send(event)}>
      <input
        aria-label="Ask anything"
        placeholder="Ask anything"
        value={text}
        onChange={(event) => setText(event.currentTarget.value)}
      />
      <button
        type="submit"
        aria-label="Send message"
        disabled={!text.trim()}
      >
        <ArrowUp size={16} aria-hidden="true" />
      </button>
    </form>
  );
}
