import { useCallback, useRef, useState } from "react";
import type { AcpGoalState } from "../../domain/acpGoal";
import { canMutateGoal } from "./acp/goalMutability";
import { currentContext } from "./acp/useAcpChatActions";

export type BoundGoalDetails = ReturnType<typeof useBoundGoalDetails>;

export function useBoundGoalDetails(
  state: AcpGoalState,
  chatId: string,
  enabled: boolean,
) {
  const detailsRef = useRef<HTMLDetailsElement>(null);
  // The Goal control loads on demand; the + menu offers Goal only once its
  // details are there to open.
  const [ready, setReady] = useState(false);
  const onReady = useCallback(() => setReady(true), []);
  const canMutate =
    state.kind === "goal" &&
    canMutateGoal(currentContext(chatId || undefined)?.page);
  const onOpen =
    enabled && canMutate && ready
      ? () => {
          if (detailsRef.current) {
            detailsRef.current.open = true;
            detailsRef.current.querySelector("summary")?.focus();
          }
        }
      : undefined;
  return { detailsRef, onOpen, canMutate, onReady };
}
