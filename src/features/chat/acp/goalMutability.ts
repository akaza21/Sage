import type { AcpChatPage } from "../../../domain/acp";
import type { GoalMutationAction } from "../../../domain/acpGoal";

export function canMutateGoal(
  page: AcpChatPage | null | undefined,
  action?: GoalMutationAction,
): boolean {
  const session = page?.sessions?.find(
    (item) => item.id === page.currentSessionId,
  );
  if (!session?.runtimeEpoch || !session.capabilities.goal) return false;
  const goal = session.capabilities.goal;
  if (!goal.observes && (action === "set" || action === "clear"))
    return goal[action];
  if (action === "set")
    return (
      goal.set && (page?.goal?.kind === "noGoal" || page?.goal?.kind === "goal")
    );
  return (
    page?.goal?.kind === "goal" &&
    (action ? goal[action] : goal.set || goal.pause || goal.clear)
  );
}
