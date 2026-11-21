import type { AcpGoalState } from "../../domain/acpGoal";
import { ChatGoalControl } from "./ChatGoalControl";
import { useGoalMutation } from "./acp/useGoalMutation";
import type { BoundGoalDetails } from "./useBoundGoalDetails";

export function BoundChatGoalControl({
  state,
  chatId,
  details: { detailsRef, canMutate, onReady, capabilities },
}: {
  state: AcpGoalState;
  chatId: string;
  details: BoundGoalDetails;
}) {
  const boundChatId = chatId || undefined;
  const mutate = useGoalMutation(boundChatId);
  return (
    <ChatGoalControl
      state={state}
      canMutate={canMutate}
      capabilities={capabilities}
      onReady={onReady}
      onMutation={mutate}
      detailsRef={detailsRef}
    />
  );
}
