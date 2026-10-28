import {
  isBrowserElement,
  MAX_ATTACHED_CONTEXTS_PER_TURN,
} from "../domain/attachedContext";
import { pinnedAgentChatOwner } from "./agentChatOwner";
import type { BrowserElementSink } from "./browserAnnotations";
import { useWorkbenchStore } from "./store";

function draft() {
  const chatId = pinnedAgentChatOwner();
  return chatId
    ? (useWorkbenchStore.getState().chatUiById[chatId]?.attachedContext ?? [])
    : null;
}

/**
 * Annotate's elements join the draft of the Agent Chat the browser pane
 * belongs to (the pinned owner), through the same ownership capture and
 * attach path as the page attachment.
 */
export const browserElementChatSink: BrowserElementSink = {
  nextOrdinal() {
    const contexts = draft();
    if (!contexts || contexts.length >= MAX_ATTACHED_CONTEXTS_PER_TURN)
      return null;
    const used = new Set(
      contexts.filter(isBrowserElement).map((element) => element.ordinal),
    );
    let ordinal = 1;
    while (used.has(ordinal)) ordinal += 1;
    return ordinal;
  },
  attach(element) {
    const chatId = pinnedAgentChatOwner();
    const state = useWorkbenchStore.getState();
    const intent = chatId ? state.captureChatOwnership(chatId) : null;
    return Boolean(intent && state.attachChatContext(intent, element));
  },
};
