import { chatDisplayTitle } from "../domain/chatTitle";
import type { WorkbenchTab } from "../domain/workbench";
import type { ChatLiveSnapshot } from "../features/chat/acp/chatLiveSnapshot";
import {
  ackChatUnread,
  chatActivityFingerprint,
} from "../features/chat/acp/chatUnread";
import { useAcpChatLiveSnapshot } from "../features/chat/acp/useAcpChatLiveSnapshot";
import { renderRegisteredCanvas } from "./surfaceRegistry";

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

  function handleFocus() {
    ackChatUnread(tab.id, activityFingerprint);
    onFocus();
  }

  // Under Agent chrome the pane has no header of its own: the task header
  // names the focused Chat, and a side chat's tab sits in the center
  // header row (Codex s2/f0244).
  return (
    <section
      className={`agent-chat-pane${focused ? " agent-chat-pane--focused" : ""}`}
      aria-label={`Chat pane ${title}`}
      data-chat-id={tab.id}
      onFocus={handleFocus}
    >
      {renderRegisteredCanvas("chat", tab)}
    </section>
  );
}
