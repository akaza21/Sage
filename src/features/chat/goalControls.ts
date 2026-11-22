export interface GoalControls {
  onOpenGoal?: () => void;
  onClearGoal?: () => void;
  goalUnobserved?: boolean;
}

export type GoalControlsChange = (controls?: GoalControls) => void;
