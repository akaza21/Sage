import { useEffect } from "react";
import { draftShownFor } from "../../app/newChatDraftSlice";
import { useWorkbenchStore } from "../../app/store";
import type {
  AcpChatPage,
  CuratedBindingId,
  CuratedModelId,
} from "../../domain/acp";
import {
  currentContext,
  type useAcpChatActions,
} from "./acp/useAcpChatActions";

export function FirstChatSend({
  chatId,
  page,
  actions,
}: {
  chatId: string;
  page: AcpChatPage | null | undefined;
  actions: ReturnType<typeof useAcpChatActions>;
}) {
  const pending = useWorkbenchStore((state) => state.pendingFirstSend);
  const transitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  useEffect(() => {
    if (!pending?.chatId || pending.chatId !== chatId || transitioning) return;
    const state = useWorkbenchStore.getState();
    if (state.pendingFirstSend !== pending) return;
    const entry = state.chatUiById[chatId];
    const harness = state.harnesses.find(
      (harness) => harness.id === entry?.harnessId,
    );
    if (harness?.transport === "acp") {
      const context = currentContext(chatId);
      if (!page || !context || context.page !== page) return;
    }
    useWorkbenchStore.setState({ pendingFirstSend: null });
    if (
      draftShownFor(state, chatId) ||
      state.activeTabId !== chatId ||
      state.project?.id !== pending.projectId ||
      state.chatOwnershipEpoch !== pending.ownershipEpoch
    )
      return;
    if (
      !entry?.composerText ||
      entry.composerText !== pending.text ||
      harness?.installState !== "available"
    )
      return;
    if (harness.transport === "legacySimulator") {
      void state.sendPrompt();
    } else if (harness.bindingId) {
      const bindingId = harness.bindingId as CuratedBindingId;
      const text = entry.composerText.trim();
      const images = pending.images ?? [];
      const send = () =>
        actions.submit(
          text,
          useWorkbenchStore.getState().chatUiById[chatId]?.attachedContext ??
            [],
          bindingId,
          (entry.modelId ?? null) as CuratedModelId | null,
        );
      const intent = state.captureChatOwnership(chatId);
      // The draft's images reach this chat once: uploaded into its composer,
      // which the send clears. A failed upload leaves them there, unsent.
      if (!images.length) void send();
      else if (intent)
        void import("./chatImageTransfer")
          .then((transfer) => transfer.uploadChatImages(intent, images))
          .then(send, (error: Error) =>
            useWorkbenchStore.getState().setChatActionState(intent, {
              sending: false,
              error: error.message,
            }),
          );
    }
  }, [actions, chatId, page, pending, transitioning]);
  return null;
}
