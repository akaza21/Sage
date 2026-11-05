import { useWorkbenchStore } from "../../app/store";
import { useChatHarnessSelection } from "./useChatHarnessSelection";
import { useChatQueueState } from "./useChatQueueState";
import { useChatSurfaceModel } from "./useChatSurfaceModel";
import { useAcpChatActions } from "./acp";
import type ChatQueueExecution from "./ChatQueueExecution";

export default function ChatQueueRuntime({
  chatId,
  Execution,
}: {
  chatId?: string;
  Execution: typeof ChatQueueExecution;
}) {
  const { page, selectedHarnessId, selectedModelId, discoveryPhase } =
    useChatHarnessSelection(chatId);
  const queue = useChatQueueState(chatId, page);
  const queuedTurnBarrier = useWorkbenchStore(
    (state) =>
      state.chatUiById[chatId ?? state.activeTabId ?? ""]?.queuedTurnBarrier ??
      null,
  );
  const knownTurnOrder = useWorkbenchStore(
    (state) =>
      state.chatUiById[chatId ?? state.activeTabId ?? ""]?.knownTurnOrder ??
      null,
  );
  const harnesses = useWorkbenchStore((state) => state.harnesses);
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const actions = useAcpChatActions(chatId);
  const { activeAcpTurn, actionBusy, canPrompt, useNativeAcp } =
    useChatSurfaceModel({
      harnesses,
      selectedHarnessId,
      selectedModelId,
      page,
      discoveryPhase,
      composerText: "",
      attachedContext: [],
      sending: actions.busy,
      chatError: actions.error,
      projectTransitioning,
      focused: false,
      acpActions: actions,
      sendPrompt: unusedSend,
    });
  return (
    <Execution
      {...queue}
      queuedTurnBarrier={queuedTurnBarrier}
      knownTurnOrder={knownTurnOrder}
      activeTurn={Boolean(activeAcpTurn)}
      actionBusy={actionBusy}
      projectTransitioning={projectTransitioning}
      canPrompt={canPrompt}
      useNativeAcp={useNativeAcp}
      submit={actions.submit}
    />
  );
}
const unusedSend = async () => undefined;
