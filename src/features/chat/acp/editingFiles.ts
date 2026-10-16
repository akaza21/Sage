import type { AcpChatPage, TurnProjection } from "../../../domain/acp";
import { itemOwnershipMatches } from "./agentEditSummary";
import { terminalItemLifecycles, terminalTurnStates } from "./statusSemantics";

export function isEditingFiles(page: AcpChatPage, turn: TurnProjection) {
  return (
    turn.sessionId === page.currentSessionId &&
    !terminalTurnStates.has(turn.state) &&
    turn.items.some(
      (item) =>
        item.kind === "fileChangeDiff" &&
        !terminalItemLifecycles.has(item.lifecycle) &&
        itemOwnershipMatches(page, turn, item),
    )
  );
}
