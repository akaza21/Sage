import { useCallback, useState } from "react";

export interface GoalControls {
  onOpenGoal?: () => void;
  onClearGoal?: () => void;
  goalUnobserved?: boolean;
}

export type BoundGoalDetails = ReturnType<typeof useBoundGoalDetails>;

export function useBoundGoalDetails(enabled: boolean) {
  const [controls, setControls] = useState<GoalControls>();
  const onReady = useCallback((actions: GoalControls) => {
    setControls(actions);
    return () => setControls(undefined);
  }, []);
  return {
    onReady,
    actions: enabled ? controls : undefined,
  };
}
