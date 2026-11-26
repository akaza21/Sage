import type { AcpChatPage, TurnProjection } from "../../../domain/acp";
import { currentOrLatestTurn, terminalTurnStates } from "./statusSemantics";

export function currentNonterminalTurn(
  page: AcpChatPage | null,
): TurnProjection | null {
  const turn = page?.currentSessionId
    ? currentOrLatestTurn(page, page.currentSessionId)
    : null;
  return turn && !terminalTurnStates.has(turn.state) ? turn : null;
}
