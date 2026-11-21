import { useCallback, useRef, useState } from "react";
import type { AcpGoalCapabilities, AcpGoalState } from "../../domain/acpGoal";

export type BoundGoalDetails = ReturnType<typeof useBoundGoalDetails>;

export function useBoundGoalDetails(
  state: AcpGoalState,
  capabilities: AcpGoalCapabilities | undefined,
  enabled: boolean,
) {
  const detailsRef = useRef<HTMLDetailsElement>(null);
  const [open, setOpen] = useState<(() => void) | undefined>();
  const onReady = useCallback((onOpen: () => void) => {
    setOpen(() => onOpen);
    return () => setOpen(undefined);
  }, []);
  const canMutate =
    !!capabilities &&
    (state.kind === "goal" || (state.kind === "noGoal" && capabilities.set));
  return {
    detailsRef,
    capabilities,
    canMutate,
    onReady,
    onOpen: enabled && canMutate ? open : undefined,
  };
}
