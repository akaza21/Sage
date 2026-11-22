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
    } else if (pending.images?.length) {
      // The draft's images load their own upload-then-send step.
      void import("./draftImages").then((draft) =>
        draft.sendDraftImages(pending, actions),
      );
    } else if (harness.bindingId) {
      void actions.submit(
        entry.composerText.trim(),
        [],
        harness.bindingId as CuratedBindingId,
        (entry.modelId ?? null) as CuratedModelId | null,
      );
    }
  }, [actions, chatId, page, pending, transitioning]);
  return null;
}
