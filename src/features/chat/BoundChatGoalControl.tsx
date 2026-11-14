import type { AcpGoalState } from "../../domain/acpGoal";
import { useEffect } from "react";
import { ChatGoalControl } from "./ChatGoalControl";
import { useGoalMutation } from "./acp/useGoalMutation";
import type { BoundGoalDetails } from "./useBoundGoalDetails";

export function BoundChatGoalControl({
  state,
  chatId,
  details: { detailsRef, canMutate, onReady },
}: {
  state: AcpGoalState;
  chatId: string;
  details: BoundGoalDetails;
}) {
  const boundChatId = chatId || undefined;
  const mutate = useGoalMutation(boundChatId);
  useEffect(onReady, [onReady]);
  return (
    <ChatGoalControl
      state={state}
      canMutate={canMutate}
      onMutation={mutate}
      detailsRef={detailsRef}
    />
  );
}
