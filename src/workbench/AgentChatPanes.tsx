import {
  Fragment,
  useCallback,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
} from "react";
import { useStore } from "zustand";

import {
  acpProjectionStoreFor,
  pinAcpProjectionIds,
} from "../app/acpProjectionRegistry";
import {
  AgentSessionChromeContext,
  pinAgentChatOwner,
  type AgentSessionChrome,
  usePinnedAgentChatOwner,
} from "../app/agentChatOwner";
import { useWorkbenchStore } from "../app/store";
import { pinUnreadAckIds } from "../features/chat/acp/chatUnread";
import { setHarnessBrowseOwner } from "../features/preview/harnessBrowse";
import type { HarnessBrowseOwner } from "../features/preview/harnessBrowse";
import { ChatSurface } from "../features/chat/ChatSurface";
import { useAcpChatLiveSnapshot } from "../features/chat/acp/useAcpChatLiveSnapshot";
import { AgentChatPane } from "./AgentChatPane";
import { AgentChatCenterHeader } from "./AgentChatCenterHeader";
import { isSideChatId, useSideChatState } from "./sideChatState";
import { TranscriptPart } from "../features/chat/transcriptLazy";
import {
  AGENT_CHAT_SPLIT_MAX,
  AGENT_CHAT_SPLIT_MIN,
  AGENT_CHAT_SPLIT_MIN_CONTAINER,
  resizeAgentChatSplit,
} from "./agentChatSplit";
import { collectChatTabIds, focusedAgentChatId } from "./workbenchWindowMode";

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
  const [resizing, setResizing] = useState(false);
  const panesRef = useRef<HTMLDivElement>(null);
  const previousFocusedId = usePinnedAgentChatOwner();
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
  const setSplitPercent = (percent: number) =>
    useSideChatState.setState({ split: percent });
  const focusCandidates =
    splitRequested && side ? [side.parentChatId, side.id] : chatIds;
  const focusedId = focusedAgentChatId(
    focusCandidates,
    activeTabId,
    previousFocusedId,
  );
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
    return () => pinAgentChatOwner(null);
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

  useLayoutEffect(() => {
    setHarnessBrowseOwner(focusedOwner, displayedIds);
  }, [displayedIds, focusedOwner]);

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

  function setSplitFromPointer(event: PointerEvent<HTMLDivElement>) {
    const bounds = panesRef.current?.getBoundingClientRect();
    if (!bounds) return;
    setSplitPercent(
      resizeAgentChatSplit({
        containerWidth: bounds.width,
        pointerOffset: event.clientX - bounds.left,
      }),
    );
  }

  function beginResize(event: PointerEvent<HTMLDivElement>) {
    event.currentTarget.setPointerCapture(event.pointerId);
    setResizing(true);
    setSplitFromPointer(event);
  }

  function endResize(event: PointerEvent<HTMLDivElement>) {
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    setResizing(false);
  }

  function resizeByKeyboard(event: KeyboardEvent<HTMLDivElement>) {
    const amount = event.shiftKey ? 10 : 5;
    const next =
      event.key === "ArrowLeft"
        ? splitPercent - amount
        : event.key === "ArrowRight"
          ? splitPercent + amount
          : event.key === "Home"
            ? AGENT_CHAT_SPLIT_MIN
            : event.key === "End"
              ? AGENT_CHAT_SPLIT_MAX
              : null;
    if (next === null) return;
    event.preventDefault();
    setSplitPercent(
      Math.min(AGENT_CHAT_SPLIT_MAX, Math.max(AGENT_CHAT_SPLIT_MIN, next)),
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
                  onFocus={() => void activateTab(id)}
                />
                {split && index === 0 ? (
                  <div
                    className={`agent-chat-splitter${
                      resizing ? " agent-chat-splitter--active" : ""
                    }`}
                    role="separator"
                    aria-label="Resize chat panes"
                    aria-orientation="vertical"
                    aria-valuemin={AGENT_CHAT_SPLIT_MIN}
                    aria-valuemax={AGENT_CHAT_SPLIT_MAX}
                    aria-valuenow={splitPercent}
                    tabIndex={0}
                    onKeyDown={resizeByKeyboard}
                    onPointerDown={beginResize}
                    onPointerMove={(event) => {
                      if (resizing) setSplitFromPointer(event);
                    }}
                    onPointerUp={endResize}
                    onPointerCancel={endResize}
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
    </AgentSessionChromeContext.Provider>
  );
}
