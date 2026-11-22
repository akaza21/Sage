import type {
  AcpMutationResult,
  AcpSessionMutationIntent,
} from "../domain/acp";
import { decodeAcpGoal } from "../domain/acpGoal";
import { invokeAcpMutation } from "./acpDesktop";

// Goal mutation commands load with their controls, separately from chat reads.
export const goalMutations = {
  pauseExistingGoal(
    request: AcpSessionMutationIntent,
  ): Promise<AcpMutationResult> {
    return invokeAcpMutation("pause_existing_goal", request, decodeAcpGoal);
  },
  setGoal(
    request: AcpSessionMutationIntent & {
      objective: string;
      tokenBudget?: number;
    },
  ): Promise<AcpMutationResult> {
    return invokeAcpMutation("set_goal", request, decodeAcpGoal);
  },
  resumeExistingGoal(
    request: AcpSessionMutationIntent,
  ): Promise<AcpMutationResult> {
    return invokeAcpMutation("resume_existing_goal", request, decodeAcpGoal);
  },
  setGoalBudget(
    request: AcpSessionMutationIntent & { tokenBudget: number },
  ): Promise<AcpMutationResult> {
    return invokeAcpMutation("set_goal_budget", request, decodeAcpGoal);
  },
  clearExistingGoal(
    request: AcpSessionMutationIntent,
  ): Promise<AcpMutationResult> {
    return invokeAcpMutation("clear_existing_goal", request, decodeAcpGoal);
  },
};
