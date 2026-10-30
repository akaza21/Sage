import {
  Fragment,
  memo,
  useCallback,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useStore } from "zustand";
import {
  acpProjectionStoreFor,
  pinAcpProjectionIds,
} from "../app/acpProjectionRegistry";
import {
  AgentSessionChromeContext,
  pinAgentChatOwner,
  releaseAgentChatOwner,
  type AgentSessionChrome,
  usePinnedAgentChatOwner,
} from "../app/agentChatOwner";
import { useWorkbenchStore } from "../app/store";
import {
  pinUnreadAckIds,
  ackChatUnread,
  chatActivityFingerprint,
} from "../features/chat/acp/chatUnread";
import {
  type HarnessBrowseOwner,
  setHarnessBrowseOwner,
} from "../features/preview/harnessBrowse";
import { ChatSurface } from "../features/chat/ChatSurface";
import { useAcpChatLiveSnapshot } from "../features/chat/acp/useAcpChatLiveSnapshot";
import { AgentChatCenterHeader } from "./AgentChatCenterHeader";
import { isSideChatId, useSideChatState } from "./sideChatState";
import { TranscriptPart } from "../features/chat/transcriptLazy";
import { lazyParts } from "../features/chat/lazyParts";
import type * as SplitterChunk from "./AgentChatSplitter";
import { AGENT_CHAT_SPLIT_MIN_CONTAINER } from "./agentChatSplit";
import { collectChatTabIds, focusedAgentChatId } from "./workbenchWindowMode";
import { chatDisplayTitle } from "../domain/chatTitle";
import { type WorkbenchTab } from "../domain/workbench";
import { type ChatLiveSnapshot } from "../features/chat/acp/chatLiveSnapshot";
import { renderRegisteredCanvas } from "./surfaceRegistry";

const SplitterPart = lazyParts<typeof SplitterChunk>(
  () => import("./AgentChatSplitter"),
);

export function AgentChatPanes({
  compact = false,
  navigationCollapsed = false,
  onArtifactHeaderMount,
}: {
  compact?: boolean;
  navigationCollapsed?: boolean;
  onArtifactHeaderMount?: (node: HTMLDivElement | null) => void;
}) {
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const activateTab = useWorkbenchStore((state) => state.activateTab);
  const project = useWorkbenchStore((state) => state.project);
  const projectGeneration = useWorkbenchStore(
    (state) => state.projectGeneration,
  );
  const nativeTransitionEpoch = useWorkbenchStore(
    (state) => state.nativeTransitionEpoch,
  );
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  // Only the focused Chat and a focused side chat's parent publish, and each
  // withdraws on cleanup, so this holds at most two entries.
  const [sessionChromeById, setSessionChromeById] = useState<
    Readonly<Record<string, AgentSessionChrome>>
  >({});
  const publishSessionChrome = useCallback(
    (chatId: string, chrome: AgentSessionChrome | null) =>
      setSessionChromeById(({ [chatId]: _withdrawn, ...rest }) =>
        chrome ? { ...rest, [chatId]: chrome } : rest,
      ),
    [],
  );
  const panesRef = useRef<HTMLDivElement>(null);
  const previousFocusedId = usePinnedAgentChatOwner();
  const [lastFocusedId, setLastFocusedId] = useState<string | null>(null);
  const chatIds = useMemo(() => collectChatTabIds(tabs), [tabs]);
  // A side chat pairs with its parent (parent left) while either has focus;
  // its 43 / 57 split lives in the side-chat store and is never persisted.
  const splitPercent = useSideChatState((state) => state.split);
  const pairedId = useSideChatState((state) => state.pairedId);
  const side = useSideChatState((state) =>
    state.sides.find(
      (entry) =>
        entry.projectId === project?.id &&
        chatIds.includes(entry.id) &&
        chatIds.includes(entry.parentChatId),
    ),
  );
  const splitRequested = Boolean(
    side &&
    pairedId === side.id &&
    (!activeTabId ||
      !chatIds.includes(activeTabId) ||
      activeTabId === side.id ||
      activeTabId === side.parentChatId),
  );
  const focusCandidates =
    splitRequested && side ? [side.parentChatId, side.id] : chatIds;
  const focusedId = focusedAgentChatId(
    focusCandidates,
    activeTabId,
    previousFocusedId ?? lastFocusedId,
  );
  if (lastFocusedId !== focusedId) setLastFocusedId(focusedId);
  const displayedIds = useMemo(
    () =>
      splitRequested && !compact && side
        ? [side.parentChatId, side.id]
        : focusedId
          ? [focusedId]
          : [],
    [compact, focusedId, side, splitRequested],
  );
  useLayoutEffect(() => {
    if (pairedId && (compact || !splitRequested)) {
      useSideChatState.setState({ pairedId: null });
    }
  }, [compact, pairedId, splitRequested]);
  pinAcpProjectionIds(displayedIds);
  pinUnreadAckIds(displayedIds);

  useLayoutEffect(() => {
    pinAgentChatOwner(focusedId);
    return releaseAgentChatOwner;
  }, [focusedId]);

  const focusedProjectionStore = acpProjectionStoreFor(focusedId);
  const focusedPage = useStore(focusedProjectionStore, (state) => state.page);
  const focusedScope = useStore(focusedProjectionStore, (state) => state.scope);
  const focusedOwner = useMemo<HarnessBrowseOwner | null>(() => {
    const session = focusedPage?.sessions.find(
      (candidate) => candidate.id === focusedPage.currentSessionId,
    );
    if (
      projectTransitioning ||
      !project ||
      !focusedPage?.currentSessionId ||
      !focusedScope ||
      !session?.runtimeEpoch ||
      !Number.isSafeInteger(nativeTransitionEpoch) ||
      nativeTransitionEpoch < 0 ||
      focusedPage.projectId !== project.id ||
      focusedPage.chatId !== focusedId ||
      focusedScope.projectId !== focusedPage.projectId ||
      focusedScope.chatId !== focusedPage.chatId ||
      focusedScope.projectGeneration !== String(projectGeneration)
    ) {
      return null;
    }
    return {
      projectId: focusedPage.projectId,
      projectGeneration: focusedScope.projectGeneration,
      transitionEpoch: nativeTransitionEpoch,
      chatId: focusedPage.chatId,
      sessionId: focusedPage.currentSessionId,
      runtimeEpoch: session.runtimeEpoch,
    };
  }, [
    focusedId,
    focusedPage,
    focusedScope,
    nativeTransitionEpoch,
    project,
    projectGeneration,
    projectTransitioning,
  ]);

  useRetainedHarnessBrowse(
    focusedOwner,
    displayedIds,
    chatIds,
    Boolean(focusedId) &&
      !projectTransitioning &&
      (!focusedScope || focusedPage?.chatId !== focusedId),
  );

  const focusedSnapshot = useAcpChatLiveSnapshot(
    focusedId ?? "",
    Boolean(focusedId),
  );
  useLayoutEffect(() => {
    const panes = panesRef.current;
    if (!panes || !splitRequested || compact) return;

    const updateWidth = (width: number) => {
      if (width > 0 && width < AGENT_CHAT_SPLIT_MIN_CONTAINER) {
        useSideChatState.setState({ pairedId: null });
      }
    };
    updateWidth(panes.getBoundingClientRect().width);

    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver((entries) => {
      updateWidth(entries[0]?.contentRect.width ?? 0);
    });
    observer.observe(panes);
    return () => observer.disconnect();
  }, [compact, splitRequested]);

  if (!focusedId) {
    return (
      <div className="active-canvas" role="tabpanel">
        <ChatSurface />
      </div>
    );
  }

  const split = displayedIds.length > 1;
  return (
    <AgentSessionChromeContext.Provider value={publishSessionChrome}>
      <div className="agent-chat-center">
        <AgentChatCenterHeader
          tabs={tabs}
          paneIds={displayedIds.some(isSideChatId) ? displayedIds : null}
          focusedId={focusedId}
          focusedSnapshot={focusedSnapshot}
          chatCount={chatIds.length}
          sessionChromeById={sessionChromeById}
          splitPercent={splitPercent}
          onArtifactHeaderMount={onArtifactHeaderMount}
          navigationCollapsed={navigationCollapsed}
        />
        <div
          ref={panesRef}
          id="agent-chat-panel"
          aria-label="Agent chat panes"
          className={`agent-chat-panes${split ? " agent-chat-panes--split" : ""}`}
          role="tabpanel"
          style={
            split
              ? {
                  gridTemplateColumns: `minmax(0, ${splitPercent}fr) 8px minmax(0, ${
                    100 - splitPercent
                  }fr)`,
                }
              : undefined
          }
        >
          {displayedIds.map((id, index) => {
            const tab = tabs.find((candidate) => candidate.id === id);
            if (!tab) return null;
            return (
              <Fragment key={id}>
                <AgentChatPane
                  tab={tab}
                  focused={id === focusedId}
                  snapshot={id === focusedId ? focusedSnapshot : undefined}
                  activateTab={activateTab}
                />
                {split && index === 0 ? (
                  <SplitterPart part="AgentChatSplitter" panesRef={panesRef} />
                ) : null}
              </Fragment>
            );
          })}
        </div>
      </div>
      {chatIds.some(isSideChatId) ? (
        <TranscriptPart part="SideChatDialogs" />
      ) : null}
    </AgentSessionChromeContext.Provider>
  );
}

/** Recently focused Chats whose harness Preview session survives a switch. */
const MAX_RETAINED_BROWSE_CHATS = 2;

/**
 * AP-A1 shows one Chat in the centre, so "displayed" alone would drop a
 * Chat's harness Preview the moment another Chat is selected. Retain the two
 * most recently focused Chats instead (an explicit side split keeps its two
 * panes), and keep the current owner while the next Chat's projection loads.
 */
function useRetainedHarnessBrowse(
  owner: HarnessBrowseOwner | null,
  displayedIds: readonly string[],
  chatIds: readonly string[],
  ownerLoading: boolean,
): void {
  const recent = useRef<string[]>([]);
  useLayoutEffect(() => {
    if (!owner) {
      if (ownerLoading) return;
      recent.current = [];
      setHarnessBrowseOwner(null);
      return;
    }
    recent.current = [
      owner.chatId,
      ...recent.current.filter(
        (id) => id !== owner.chatId && chatIds.includes(id),
      ),
    ].slice(0, MAX_RETAINED_BROWSE_CHATS);
    setHarnessBrowseOwner(
      owner,
      displayedIds.length > 1 ? displayedIds : recent.current,
    );
  }, [owner, displayedIds, chatIds, ownerLoading]);
}

/** Memoized so pane-host state (session chrome, focus memory) does not re-render the transcript. */
const AgentChatPane = memo(function AgentChatPane({
  tab,
  focused,
  activateTab,
  snapshot: providedSnapshot,
}: {
  tab: WorkbenchTab;
  focused: boolean;
  activateTab: (tabId: string) => Promise<void>;
  snapshot?: ChatLiveSnapshot;
}) {
  const ownSnapshot = useAcpChatLiveSnapshot(tab.id, !providedSnapshot);
  const snapshot = providedSnapshot ?? ownSnapshot;
  const title = chatDisplayTitle(tab.title);
  const activityFingerprint = chatActivityFingerprint(snapshot);
  if (focused && activityFingerprint.length > 0) {
    ackChatUnread(tab.id, activityFingerprint);
  }

  function handleFocus() {
    ackChatUnread(tab.id, activityFingerprint);
    void activateTab(tab.id);
  }

  // Under Agent chrome the pane has no header of its own: the task header
  // names the focused Chat, and a side chat's tab sits in the center
  // header row (Codex s2/f0244).
  return (
    <section
      className={`agent-chat-pane${focused ? " agent-chat-pane--focused" : ""}`}
      aria-label={`Chat pane ${title}`}
      data-chat-id={tab.id}
      tabIndex={-1}
      onFocus={handleFocus}
    >
      {renderRegisteredCanvas("chat", tab)}
    </section>
  );
});
