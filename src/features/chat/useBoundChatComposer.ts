import { useWorkbenchStore } from "../../app/store";

export function useBoundChatComposer(chatId?: string) {
  const composerText = useWorkbenchStore((state) => {
    if (!chatId) return state.composerText;
    return (
      state.chatUiById[chatId]?.composerText ??
      (state.activeTabId === chatId ? state.composerText : "")
    );
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

  async function sendBoundPrompt() {
    if (chatId && activeTabId !== chatId) {
      await activateTab(chatId);
    }
    await sendPrompt();
  }

  return {
    composerText,
    sending,
    chatError,
    setComposerText: setBoundComposerText,
    sendPrompt: sendBoundPrompt,
  };
}
