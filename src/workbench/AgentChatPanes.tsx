import {
  Fragment,
  useEffect,
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
  pinAgentChatOwner,
  usePinnedAgentChatOwner,
} from "../app/agentChatOwner";
import { useWorkbenchStore } from "../app/store";
import { pinUnreadAckIds } from "../features/chat/acp/chatUnread";
import { setHarnessBrowseOwner } from "../features/preview/harnessBrowse";
import type { HarnessBrowseOwner } from "../features/preview/harnessBrowse";
import { ChatSurface } from "../features/chat/ChatSurface";
import { AgentChatPane } from "./AgentChatPane";
import {
  AGENT_CHAT_SPLIT_MAX,
  AGENT_CHAT_SPLIT_MIN,
  canShowAgentChatSplit,
  resizeAgentChatSplit,
} from "./agentChatSplit";
import {
  readWorkbenchLayout,
  writeWorkbenchLayout,
} from "./workbenchLayoutStorage";
import {
  collectChatTabIds,
  focusedAgentChatId,
  reconcileVisibleChatIds,
  selectVisibleChatIds,
} from "./workbenchWindowMode";

function sameVisibleIds(left: readonly string[], right: readonly string[]) {
  if (left.length !== right.length) return false;
  for (let index = 0; index < left.length; index += 1) {
    if (left[index] !== right[index]) return false;
  }
  return true;
}

export function AgentChatPanes({ compact = false }: { compact?: boolean }) {
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
  const [visibleIds, setVisibleIds] = useState<string[]>([]);
  const [splitPercent, setSplitPercent] = useState(
    () => readWorkbenchLayout().agentChatSplitPercent,
  );
  const [resizing, setResizing] = useState(false);
  const [containerWidth, setContainerWidth] = useState<number | null>(null);
  const panesRef = useRef<HTMLDivElement>(null);
  const previousFocusedId = usePinnedAgentChatOwner();
  const chatIds = useMemo(() => collectChatTabIds(tabs), [tabs]);
  const nextVisibleIds =
    activeTabId && chatIds.includes(activeTabId)
      ? selectVisibleChatIds(chatIds, visibleIds, activeTabId)
      : reconcileVisibleChatIds(chatIds, visibleIds, null);
  if (!sameVisibleIds(nextVisibleIds, visibleIds)) {
    setVisibleIds(nextVisibleIds);
  }

  const focusedId = focusedAgentChatId(
    chatIds,
    nextVisibleIds,
    activeTabId,
    previousFocusedId,
  );
  const focusOnly =
    compact ||
    (nextVisibleIds.length > 1 &&
      containerWidth !== null &&
      !canShowAgentChatSplit(containerWidth));
  const displayedIds = useMemo(
    () => (focusOnly && focusedId ? [focusedId] : nextVisibleIds),
    [focusOnly, focusedId, nextVisibleIds],
  );
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
      projectGeneration !== nativeTransitionEpoch ||
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

  const hasVisibleChats = nextVisibleIds.length > 0;
  useLayoutEffect(() => {
    const panes = panesRef.current;
    if (!panes || !hasVisibleChats) return;

    const updateWidth = (width: number) => {
      if (Number.isFinite(width) && width > 0) {
        setContainerWidth(width);
      }
    };
    updateWidth(panes.getBoundingClientRect().width);

    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver((entries) => {
      const width = entries[0]?.contentRect.width;
      if (width !== undefined) updateWidth(width);
    });
    observer.observe(panes);
    return () => observer.disconnect();
  }, [hasVisibleChats]);

  useEffect(() => {
    writeWorkbenchLayout({ agentChatSplitPercent: splitPercent });
  }, [splitPercent]);

  if (nextVisibleIds.length === 0) {
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
    <div
      ref={panesRef}
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
  );
}
