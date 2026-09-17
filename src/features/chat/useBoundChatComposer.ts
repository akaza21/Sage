import { useWorkbenchStore } from "../../app/store";
import { attachedContextKey } from "../../domain/attachedContext";

const EMPTY_ATTACHED_CONTEXT = [] as const;

export function useBoundChatComposer(chatId?: string) {
  const composerText = useWorkbenchStore((state) => {
    if (!chatId) return state.composerText;
    return (
      state.chatUiById[chatId]?.composerText ??
      (state.activeTabId === chatId ? state.composerText : "")
    );
  });
  const attachedContext = useWorkbenchStore((state) => {
    const ownerId = chatId ?? state.activeTabId;
    return ownerId
      ? (state.chatUiById[ownerId]?.attachedContext ?? EMPTY_ATTACHED_CONTEXT)
      : EMPTY_ATTACHED_CONTEXT;
  });
  const sending = useWorkbenchStore((state) =>
    chatId ? (state.chatUiById[chatId]?.sending ?? false) : state.sending,
  );
  const chatError = useWorkbenchStore((state) =>
    chatId ? (state.chatUiById[chatId]?.error ?? null) : state.chatError,
  );
  const setComposerText = useWorkbenchStore((state) => state.setComposerText);
  const captureChatOwnership = useWorkbenchStore(
    (state) => state.captureChatOwnership,
  );
  const setChatComposer = useWorkbenchStore((state) => state.setChatComposer);
  const detachChatContext = useWorkbenchStore(
    (state) => state.detachChatContext,
  );
  const sendPrompt = useWorkbenchStore((state) => state.sendPrompt);
  const activateTab = useWorkbenchStore((state) => state.activateTab);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);

  function setBoundComposerText(text: string) {
    if (chatId) {
      const intent = captureChatOwnership(chatId);
      if (intent) {
        setChatComposer(intent, text);
        return;
      }
    }
    setComposerText(text);
  }

  function removeAttachedContext(contextKey: string) {
    const owner = chatId ?? activeTabId ?? undefined;
    const intent = owner ? captureChatOwnership(owner) : null;
    if (intent) detachChatContext(intent, contextKey);
  }

  async function sendBoundPrompt() {
    if (chatId && activeTabId !== chatId) {
      await activateTab(chatId);
    }
    await sendPrompt();
  }

  return {
    composerText,
    attachedContext,
    sending,
    chatError,
    setComposerText: setBoundComposerText,
    removeAttachedContext,
    attachedContextKey,
    sendPrompt: sendBoundPrompt,
  };
}
