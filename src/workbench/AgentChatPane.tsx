import { memo, useMemo } from "react";
import { useStore } from "zustand";
import { acpProjectionStoreFor } from "../app/acpProjectionRegistry";
import { chatDisplayTitle } from "../domain/chatTitle";
import { type WorkbenchTab } from "../domain/workbench";
import {
  projectChatLiveSnapshot,
  type ChatLiveSnapshot,
} from "../features/chat/acp/chatLiveSnapshot";
import {
  ackChatUnread,
  chatActivityFingerprint,
} from "../features/chat/acp/chatUnread";
import { renderRegisteredCanvas } from "./surfaceRegistry";

/** Memoized so pane-host state (session chrome, focus memory) does not re-render the transcript. */
export const AgentChatPane = memo(function AgentChatPane({
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
  const page = useStore(acpProjectionStoreFor(tab.id), (state) =>
    providedSnapshot ? null : state.page,
  );
  const snapshot = useMemo(
    () => providedSnapshot ?? projectChatLiveSnapshot(tab.id, page),
    [providedSnapshot, tab.id, page],
  );
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
  // header row.
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
