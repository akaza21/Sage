import { useState } from "react";
import { pinnedAcpProjectionIds } from "../../app/acpProjectionRegistry";
import { useWorkbenchStore } from "../../app/store";
import { useAcpChatActions, useAcpChatLoadFailure } from "../chat/acp";
import { useChatHarnessSelection } from "../chat/useChatHarnessSelection";
import { useBoundChatComposer } from "../chat/useBoundChatComposer";
import { useChatSurfaceModel } from "../chat/useChatSurfaceModel";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";

const loadQueueExecution = () => import("../chat/ChatQueueExecution");

function ChatExecution({ chatId }: { chatId: string }) {
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const selection = useChatHarnessSelection(chatId);
  const composer = useBoundChatComposer(chatId, selection.page);
  const actions = useAcpChatActions(chatId);
  const failure = useAcpChatLoadFailure(chatId);
  const model = useChatSurfaceModel({
    harnesses,
    selectedHarnessId: selection.selectedHarnessId,
    selectedModelId: selection.selectedModelId,
    page: selection.page,
    discoveryPhase: selection.discoveryPhase,
    projectTransitioning,
    focused: false,
    acpActions: actions,
    sendPrompt: composer.sendPrompt,
    composerText: composer.composerText,
    attachedContext: composer.attachedContext,
    sending: composer.sending,
    chatError: composer.chatError,
    chatLoadFailed: Boolean(failure.error),
  });
  const execution = {
    queuedPrompts: composer.queuedPrompts,
    queuedTurnBarrier: composer.queuedTurnBarrier,
    knownTurnOrder: composer.knownTurnOrder,
    activeTurn: Boolean(model.activeAcpTurn),
    actionBusy: model.actionBusy,
    projectTransitioning,
    canPrompt: model.canPrompt,
    useNativeAcp: model.useNativeAcp,
    observedOrder: composer.observedOrder,
    submit: actions.submit,
    updateQueuedPrompt: composer.updateQueuedPrompt,
    setQueuedTurnBarrier: composer.setQueuedTurnBarrier,
  };
  return composer.queuedPrompts.length || composer.queuedTurnBarrier ? (
    <LazyModuleBoundary
      label="Queued messages"
      load={loadQueueExecution}
      fallback={null}
      render={(Execution) => <Execution {...execution} />}
    />
  ) : null;
}

export default function AgentSettingsExecution() {
  const [chatIds] = useState(pinnedAcpProjectionIds);
  const tabs = useWorkbenchStore((state) => state.tabs);
  const entries = useWorkbenchStore((state) => state.chatUiById);
  return chatIds
    .filter(
      (id) =>
        (entries[id]?.queuedPrompts?.length ||
          entries[id]?.queuedTurnBarrier) &&
        tabs.some((tab) => tab.id === id && tab.kind === "chat"),
    )
    .map((chatId) => <ChatExecution key={chatId} chatId={chatId} />);
}
