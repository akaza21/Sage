import { useWorkbenchStore, type WorkbenchState } from "../../app/store";
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
  const owned = (state: WorkbenchState) =>
    state.chatUiById[chatId ?? state.activeTabId ?? ""];
  const queuedTurnBarrier = useWorkbenchStore(
    (state) => owned(state)?.queuedTurnBarrier ?? null,
  );
  const knownTurnOrder = useWorkbenchStore(
    (state) => owned(state)?.knownTurnOrder ?? null,
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
  const last = page?.turns.at(-1);
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
      hold={last?.terminalReason?.kind === "authRequired"}
      submit={actions.submit}
    />
  );
}
const unusedSend = async () => undefined;
