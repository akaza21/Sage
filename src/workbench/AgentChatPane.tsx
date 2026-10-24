import { chatDisplayTitle } from "../domain/chatTitle";
import type { WorkbenchTab } from "../domain/workbench";
import type { ChatLiveSnapshot } from "../features/chat/acp/chatLiveSnapshot";
import {
  ackChatUnread,
  chatActivityFingerprint,
  seenChatUnreadFingerprint,
  snapshotIsLive,
  unreadDotVisible,
} from "../features/chat/acp/chatUnread";
import { useAcpChatLiveSnapshot } from "../features/chat/acp/useAcpChatLiveSnapshot";
import { renderRegisteredCanvas } from "./surfaceRegistry";
import { isSideChatId } from "./sideChatState";
import { TranscriptPart } from "../features/chat/transcriptLazy";

export function AgentChatPane({
  tab,
  focused,
  onFocus,
  snapshot: providedSnapshot,
}: {
  tab: WorkbenchTab;
  focused: boolean;
  onFocus: () => void;
  snapshot?: ChatLiveSnapshot;
}) {
  const ownSnapshot = useAcpChatLiveSnapshot(tab.id, !providedSnapshot);
  const snapshot = providedSnapshot ?? ownSnapshot;
  const title = chatDisplayTitle(tab.title);
  const activityFingerprint = chatActivityFingerprint(snapshot);
  if (focused && activityFingerprint.length > 0) {
    ackChatUnread(tab.id, activityFingerprint);
  }
  const unread = unreadDotVisible({
    focused,
    live: snapshotIsLive(snapshot),
    activityFingerprint,
    seenFingerprint: seenChatUnreadFingerprint(tab.id),
  });

  function handleFocus() {
    ackChatUnread(tab.id, activityFingerprint);
    onFocus();
  }

  return (
    <section
      className={`agent-chat-pane${focused ? " agent-chat-pane--focused" : ""}`}
      aria-label={`Chat pane ${title}`}
      onFocus={handleFocus}
    >
      {renderRegisteredCanvas("chat", tab, {
        paneHeader: isSideChatId(tab.id) ? (
          <TranscriptPart
            part="SideChatHeader"
            chatId={tab.id}
            running={snapshot.working}
          />
        ) : (
          <div className="agent-chat-pane__identity">
            <button
              className="agent-chat-pane__focus"
              type="button"
              aria-pressed={focused}
              onClick={handleFocus}
            >
              {unread ? (
                <span
                  className="agent-unread-dot"
                  aria-label="Unread updates"
                />
              ) : null}
              <span className="agent-chat-pane__title">{title}</span>
            </button>
            <span
              className={`agent-chat-chip agent-chat-chip--${snapshot.statusTone}`}
              role="status"
            >
              {snapshot.statusLabel}
            </span>
          </div>
        ),
      })}
    </section>
  );
}
