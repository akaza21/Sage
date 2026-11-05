import { useWorkbenchStore } from "../../app/store";
import type { AcpChatPage } from "../../domain/acp";
import { useChatImageAttachments } from "./useChatImageAttachments";

import { useChatQueueState } from "./useChatQueueState";

const EMPTY_ATTACHED_CONTEXT = [] as const;
export { laterTurnOrder } from "./useChatQueueState";

export function useBoundChatComposer(
  chatId?: string,
  page: AcpChatPage | null = null,
) {
  const queue = useChatQueueState(chatId, page);
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
  const sendPrompt = useWorkbenchStore((state) => state.sendPrompt);
  const activateTab = useWorkbenchStore((state) => state.activateTab);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);

  const { imagePreviews, imageUploading, addImages, removeAttachedContext } =
    useChatImageAttachments(chatId, attachedContext);

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
    ...queue,
    composerText,
    attachedContext,
    imagePreviews,
    imageUploading,
    addImages,
    sending,
    chatError,
    setComposerText: setBoundComposerText,
    removeAttachedContext,
    sendPrompt: sendBoundPrompt,
  };
}
