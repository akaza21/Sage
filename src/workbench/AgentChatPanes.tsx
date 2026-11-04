import {
  Activity,
  Fragment,
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
  type AgentSessionChrome,
  AgentSessionChromeContext,
  pinAgentChatOwner,
  releaseAgentChatOwner,
  usePinnedAgentChatOwner,
} from "../app/agentChatOwner";
import { useWorkbenchStore } from "../app/store";
import { ChatSurface } from "../features/chat/ChatSurface";
import { pinUnreadAckIds } from "../features/chat/acp/chatUnread";
import { useAcpChatLiveSnapshot } from "../features/chat/acp/useAcpChatLiveSnapshot";
import { lazyParts } from "../features/chat/lazyParts";
import { TranscriptPart } from "../features/chat/transcriptLazy";
import { type HarnessBrowseOwner } from "../features/preview/harnessBrowse";
import { AgentChatCenterHeader } from "./AgentChatCenterHeader";
import { AgentChatPane } from "./AgentChatPane";
import type * as SplitterChunk from "./AgentChatSplitter";
import { canShowAgentChatSplit } from "./agentChatSplit";
import { isSideChatId, useSideChatState } from "./sideChatState";
import { useRetainedHarnessBrowse } from "./useRetainedHarnessBrowse";
import { collectChatTabIds, focusedAgentChatId } from "./workbenchWindowMode";

const SplitterPart = lazyParts<typeof SplitterChunk>(
  () => import("./AgentChatSplitter"),
);

export function AgentChatPanes({
  visible = true,
  compact = false,
  navigationCollapsed = false,
  onArtifactHeaderMount,
}: {
  visible?: boolean;
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
  const [narrow, setNarrow] = useState(false);
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
      splitRequested && !compact && !narrow && side
        ? [side.parentChatId, side.id]
        : focusedId
          ? [focusedId]
          : [],
    [compact, narrow, focusedId, side, splitRequested],
  );
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
    Boolean(focusedId) && visible,
  );
  useLayoutEffect(() => {
    const panes = panesRef.current;
    if (!visible || !panes || !splitRequested || compact) return;

    const updateWidth = (width: number) => {
      if (width > 0) setNarrow(!canShowAgentChatSplit(width));
    };
    updateWidth(panes.getBoundingClientRect().width);

    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver((entries) => {
      updateWidth(entries[0]?.contentRect.width ?? 0);
    });
    observer.observe(panes);
    return () => observer.disconnect();
  }, [compact, splitRequested, visible]);

  if (!focusedId) {
    return (
      <Activity mode={visible ? "visible" : "hidden"}>
        <div className="active-canvas" role="tabpanel">
          <ChatSurface />
        </div>
      </Activity>
    );
  }

  const split = displayedIds.length > 1;
  return (
    <AgentSessionChromeContext.Provider value={publishSessionChrome}>
      <Activity mode={visible ? "visible" : "hidden"}>
        <div className="agent-chat-center surface surface--card">
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
                    <SplitterPart
                      part="AgentChatSplitter"
                      panesRef={panesRef}
                    />
                  ) : null}
                </Fragment>
              );
            })}
          </div>
        </div>
        {chatIds.some(isSideChatId) ? (
          <TranscriptPart part="SideChatDialogs" />
        ) : null}
      </Activity>
    </AgentSessionChromeContext.Provider>
  );
}
