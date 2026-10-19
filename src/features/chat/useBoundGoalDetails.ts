import { useRef } from "react";
import type { AcpGoalState } from "../../domain/acpGoal";
import { canMutateGoal } from "./acp/useGoalMutation";
import { currentContext } from "./acp/useAcpChatActions";

export function useBoundGoalDetails(
  state: AcpGoalState,
  chatId: string,
  enabled: boolean,
) {
  const detailsRef = useRef<HTMLDetailsElement>(null);
  const canMutate =
    state.kind === "goal" &&
    canMutateGoal(currentContext(chatId || undefined)?.page);
  const onOpen =
    enabled && canMutate
      ? () => {
          if (detailsRef.current) {
            detailsRef.current.open = true;
            detailsRef.current.querySelector("summary")?.focus();
          }
        }
      : undefined;
  return { detailsRef, onOpen, canMutate };
}
