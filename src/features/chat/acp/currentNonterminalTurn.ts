import type { AcpChatPage, TurnProjection } from "../../../domain/acp";
import { terminalTurnStates } from "./statusSemantics";

export function currentNonterminalTurn(
  page: AcpChatPage | null,
): TurnProjection | null {
  const sessionId = page?.currentSessionId;
  if (!sessionId) return null;
  let current: TurnProjection | null = null;
  for (const turn of page?.turns ?? []) {
    if (
      turn.sessionId === sessionId &&
      !terminalTurnStates.has(turn.state) &&
      (!current || turn.ordinal > current.ordinal)
    ) {
      current = turn;
    }
  }
  return current;
}

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
