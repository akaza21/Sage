import type { AcpChatPage } from "../../../domain/acp";

// The backend composes a Goal only when the binding supports native Goals, so
// its presence is the capability check.
export function canMutateGoal(page: AcpChatPage | null | undefined): boolean {
  const session = page?.sessions.find(
    (item) => item.id === page.currentSessionId,
  );
  return page?.goal?.kind === "goal" && !!session?.runtimeEpoch;
}
