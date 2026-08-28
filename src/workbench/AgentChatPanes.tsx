import {
  Fragment,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
} from "react";

import { pinAcpProjectionIds } from "../app/acpProjectionRegistry";
import { pinAgentChatOwner } from "../app/agentChatOwner";
import { useWorkbenchStore } from "../app/store";
import { pinUnreadAckIds } from "../features/chat/acp/chatUnread";
import { setHarnessBrowseOwner } from "../features/preview/harnessBrowse";
import { ChatSurface } from "../features/chat/ChatSurface";
import { AgentChatPane } from "./AgentChatPane";
import {
  AGENT_CHAT_SPLIT_MAX,
  AGENT_CHAT_SPLIT_MIN,
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

export function AgentChatPanes() {
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const activateTab = useWorkbenchStore((state) => state.activateTab);
  const [visibleIds, setVisibleIds] = useState<string[]>([]);
  const [splitPercent, setSplitPercent] = useState(
    () => readWorkbenchLayout().agentChatSplitPercent,
  );
  const [resizing, setResizing] = useState(false);
  const panesRef = useRef<HTMLDivElement>(null);
  const chatIds = useMemo(() => collectChatTabIds(tabs), [tabs]);
  const nextVisibleIds =
    activeTabId && chatIds.includes(activeTabId)
      ? selectVisibleChatIds(chatIds, visibleIds, activeTabId)
      : reconcileVisibleChatIds(chatIds, visibleIds, null);
  if (!sameVisibleIds(nextVisibleIds, visibleIds)) {
    setVisibleIds(nextVisibleIds);
  }

  pinAcpProjectionIds(nextVisibleIds);
  pinUnreadAckIds(nextVisibleIds);

  const focusedId = focusedAgentChatId(chatIds, nextVisibleIds, activeTabId);
  pinAgentChatOwner(focusedId);

  useEffect(() => {
    setHarnessBrowseOwner(focusedId);
    return () => setHarnessBrowseOwner(null);
  }, [focusedId]);

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

  const split = nextVisibleIds.length > 1;
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
      {nextVisibleIds.map((id, index) => {
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
