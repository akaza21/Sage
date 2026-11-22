import type { AcpGoalState, AcpGoalCapabilities } from "../../domain/acpGoal";
import { ChatGoalControl } from "./ChatGoalControl";
import { useGoalMutation } from "./acp/useGoalMutation";
import type { BoundGoalDetails } from "./useBoundGoalDetails";

export function BoundChatGoalControl({
  state,
  chatId,
  capabilities,
  details: { onReady },
}: {
  state: AcpGoalState;
  capabilities?: AcpGoalCapabilities;
  chatId: string;
  details: BoundGoalDetails;
}) {
  const boundChatId = chatId || undefined;
  const mutate = useGoalMutation(boundChatId);
  return (
    <ChatGoalControl
      state={state}
      canMutate={!!capabilities}
      capabilities={capabilities}
      onReady={onReady}
      onMutation={mutate}
    />
  );
}
