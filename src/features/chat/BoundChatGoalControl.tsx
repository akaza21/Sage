import type { AcpGoalState, AcpGoalCapabilities } from "../../domain/acpGoal";
import { ChatGoalControl } from "./ChatGoalControl";
import { useGoalMutation } from "./acp/useGoalMutation";
import type { GoalControlsChange } from "./goalControls";

export function BoundChatGoalControl({
  state,
  chatId,
  capabilities,
  onControlsChange,
}: {
  state: AcpGoalState;
  capabilities?: AcpGoalCapabilities;
  chatId: string;
  onControlsChange: GoalControlsChange;
}) {
  const boundChatId = chatId || undefined;
  const mutate = useGoalMutation(boundChatId);
  return (
    <ChatGoalControl
      state={state}
      canMutate={!!capabilities}
      capabilities={capabilities}
      onControlsChange={onControlsChange}
      onMutation={mutate}
    />
  );
}
