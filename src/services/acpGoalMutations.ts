import type {
  AcpMutationResult,
  AcpSessionMutationIntent,
} from "../domain/acp";
import { decodeAcpGoal, type GoalIdentity } from "../domain/acpGoal";
import { invokeAcpMutation } from "./acpDesktop";

type GoalEditIntent = AcpSessionMutationIntent & {
  expectedGoal?: GoalIdentity | null;
};

// Goal mutation commands load with their controls, separately from chat reads.
function mutation<Request extends AcpSessionMutationIntent>(command: string) {
  return (request: Request): Promise<AcpMutationResult> =>
    invokeAcpMutation(command, request, decodeAcpGoal);
}

export const goalMutations = {
  pauseExistingGoal: mutation<AcpSessionMutationIntent>("pause_existing_goal"),
  setGoal: mutation<
    GoalEditIntent & { objective: string; tokenBudget?: number }
  >("set_goal"),
  resumeExistingGoal: mutation<AcpSessionMutationIntent>(
    "resume_existing_goal",
  ),
  setGoalBudget: mutation<GoalEditIntent & { tokenBudget: number }>(
    "set_goal_budget",
  ),
  clearExistingGoal: mutation<AcpSessionMutationIntent>("clear_existing_goal"),
};
