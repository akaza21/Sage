import type { AcpGoalSnapshot } from "../../domain/acpGoal";

export function isResumableGoalStatus(
  status: AcpGoalSnapshot["status"],
): boolean {
  switch (status) {
    case "paused":
    case "blocked":
    case "usageLimited":
    case "budgetLimited":
      return true;
    default:
      return false;
  }
}
