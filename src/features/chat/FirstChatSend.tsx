import { useEffect } from "react";
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
    if (!pending || pending.chatId !== chatId || !page || transitioning) return;
    const state = useWorkbenchStore.getState();
    if (state.pendingFirstSend !== pending) return;
    const context = currentContext(chatId);
    if (!context || context.page !== page) return;
    useWorkbenchStore.setState({ pendingFirstSend: null });
    if (
      state.agentCenter !== "chat" ||
      state.activeTabId !== chatId ||
      state.project?.id !== pending.projectId ||
      state.chatOwnershipEpoch !== pending.ownershipEpoch
    )
      return;
    const entry = state.chatUiById[chatId];
    const harness = state.harnesses.find(
      (harness) => harness.id === entry?.harnessId,
    );
    if (entry?.composerText && harness?.bindingId)
      void actions.submit(
        entry.composerText.trim(),
        [],
        harness.bindingId as CuratedBindingId,
        (entry.modelId ?? null) as CuratedModelId | null,
      );
  }, [actions, chatId, page, pending, transitioning]);
  return null;
}
