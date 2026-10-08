import type { AcpGoalState } from "../../domain/acpGoal";
import { ChatGoalControl } from "./ChatGoalControl";
import { canMutateGoal, useGoalMutation } from "./acp/useGoalMutation";
import { currentContext } from "./acp/useAcpChatActions";

export function BoundChatGoalControl({
  state,
  chatId,
}: {
  state: AcpGoalState;
  chatId: string;
}) {
  const boundChatId = chatId || undefined;
  const mutate = useGoalMutation(boundChatId);
  const canMutate =
    state.kind === "goal" && canMutateGoal(currentContext(boundChatId)?.page);
  return (
    <ChatGoalControl state={state} canMutate={canMutate} onMutation={mutate} />
  );
}
