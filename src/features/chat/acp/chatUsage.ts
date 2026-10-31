import type { AcpChatPage, TurnProjection } from "../../../domain/acp";

export type ChatUsage = NonNullable<TurnProjection["usage"]>;

export function currentChatUsage(page: AcpChatPage | null): ChatUsage | null {
  if (!page?.currentSessionId) return null;
  let current: ChatUsage | null = null;
  for (const turn of page.turns) {
    if (turn.sessionId !== page.currentSessionId) continue;
    if (turn.usage) current = turn.usage;
  }
  return current;
}
