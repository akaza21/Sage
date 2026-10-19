import type { AcpGoalState } from "../../domain/acpGoal";
import type { RefObject } from "react";
import { ChatGoalControl } from "./ChatGoalControl";
import { useGoalMutation } from "./acp/useGoalMutation";

export function BoundChatGoalControl({
  state,
  chatId,
  detailsRef,
  canMutate,
}: {
  state: AcpGoalState;
  chatId: string;
  detailsRef?: RefObject<HTMLDetailsElement | null>;
  canMutate: boolean;
}) {
  const boundChatId = chatId || undefined;
  const mutate = useGoalMutation(boundChatId);
  return (
    <ChatGoalControl
      state={state}
      canMutate={canMutate}
      onMutation={mutate}
      detailsRef={detailsRef}
    />
  );
}
