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
    request: AcpSessionMutationIntent & { objective: string },
  ): Promise<AcpMutationResult> {
    return invokeAcpMutation("set_goal", request, decodeAcpGoal);
  },
  clearExistingGoal(
    request: AcpSessionMutationIntent,
  ): Promise<AcpMutationResult> {
    return invokeAcpMutation("clear_existing_goal", request, decodeAcpGoal);
  },
};
