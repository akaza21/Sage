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
  // The number was read when the comment box opened, perhaps for another
  // Chat or before another tab took it; a taken number is refused, never
  // merged, so no comment is lost.
  attach(element) {
    const chatId = pinnedAgentChatOwner();
    const state = useWorkbenchStore.getState();
    const intent = chatId ? state.captureChatOwnership(chatId) : null;
    const taken = draft()
      ?.filter(isBrowserElement)
      .some(({ ordinal }) => ordinal === element.ordinal);
    return Boolean(
      intent && !taken && state.attachChatContext(intent, element),
    );
  },
};
