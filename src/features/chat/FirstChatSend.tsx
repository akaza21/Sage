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
    // A held first send (see sendFirstCommand) is already on its way.
    if (pending?.chatId !== chatId || transitioning || pending.held) return;
    const state = useWorkbenchStore.getState();
    if (state.pendingFirstSend !== pending) return;
    const entry = state.chatUiById[chatId];
    const harness = state.harnesses.find(
      (harness) => harness.id === entry?.harnessId,
    );
    if (
      harness?.transport === "acp" &&
      (!page || currentContext(chatId)?.page !== page)
    )
      return;
    useWorkbenchStore.setState({ pendingFirstSend: null });
    if (
      draftShownFor(state, chatId) ||
      state.activeTabId !== chatId ||
      state.project?.id !== pending.projectId ||
      state.chatOwnershipEpoch !== pending.ownershipEpoch
    )
      return;
    // The draft's text is never empty, so this also needs a composer entry.
    if (
      entry?.composerText !== pending.text ||
      harness?.installState !== "available"
    )
      return;
    if (harness.transport === "legacySimulator") {
      void state.sendPrompt();
    } else if (harness.bindingId) {
      // Images a draft handed over are already in this composer.
      const submit = () =>
        actions.submit(
          pending.text.trim(),
          entry.attachedContext,
          harness.bindingId as CuratedBindingId,
          (entry.modelId ?? null) as CuratedModelId | null,
        );
      void import("./ComposerSlashController").then((slash) =>
        slash.sendFirstCommand(chatId, harness.id, pending, submit),
      );
    }
  }, [actions, chatId, page, pending, transitioning]);
  return null;
}
