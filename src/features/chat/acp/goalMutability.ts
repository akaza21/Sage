import type { AcpChatPage } from "../../../domain/acp";

/**
 * The backend composes a Goal only for a current session whose binding
 * supports native Goals, so a Goal on the page is that capability; the
 * session must also be live to take a mutation.
 */
export function canMutateGoal(page: AcpChatPage | null | undefined): boolean {
  const session = page?.sessions.find(
    (item) => item.id === page.currentSessionId,
  );
  return page?.goal?.kind === "goal" && !!session?.runtimeEpoch;
}
