import type { AcpChatPage, TurnProjection } from "../../../domain/acp";

export type ChatUsage = NonNullable<TurnProjection["usage"]>;

/** A zero count clears the context: a compaction whose size no model call has stated yet. */
export function currentChatUsage(page: AcpChatPage | null): ChatUsage | null {
  if (!page?.currentSessionId) return null;
  let current: ChatUsage | null = null;
  for (const turn of page.turns) {
    if (turn.sessionId !== page.currentSessionId || !turn.usage) continue;
    current = turn.usage.totalTokens === 0 ? null : turn.usage;
  }
  return current;
}
