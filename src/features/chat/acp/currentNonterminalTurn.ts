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
