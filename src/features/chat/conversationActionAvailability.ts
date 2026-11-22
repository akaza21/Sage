import type { AcpChatPage, TurnProjection } from "../../domain/acp";
import { isSideChatId } from "../../workbench/sideChatState";

/** Only an open menu folds the bounded page; closed rows retain no summary. */
export function conversationState(page: AcpChatPage) {
  const session = page.sessions.find(({ id }) => id === page.currentSessionId);
  const offer = session?.capabilities?.rewind;
  let boundary = -1;
  page.turns.forEach((row) => {
    if (
      row.sessionId === page.currentSessionId &&
      (row.items.some((item) => item.kind === "compactionNotice") ||
        row.sessionActivities?.some(
          (activity) =>
            activity.kind === "compaction" && activity.state === "completed",
        ))
    )
      boundary = row.ordinal;
  });
  return {
    offer,
    boundary,
    hot: session?.residency === "hot",
    sessionId: page.currentSessionId,
    busy: page.turns.some((row) => row.terminalAt == null),
    latest: page.turns.at(-1)?.id,
  };
}

/** Only currently loaded, finished prompts after the newest compaction can act. */
export function conversationActions(
  page: AcpChatPage,
  turn: TurnProjection,
  state = conversationState(page),
) {
  const { offer, boundary, hot, sessionId, busy, latest } = state;
  const available =
    offer &&
    turn.conversationTarget === true &&
    turn.ordinal > boundary &&
    page.turns.some((row) => row.id === turn.id) &&
    sessionId === turn.sessionId &&
    hot &&
    turn.terminalAt != null &&
    !busy &&
    (offer.targets !== "latestTurn" || turn.id === latest);
  return {
    rewind: !!(available && offer.effects.conversation),
    fork: !!(available && offer.forkAt && !isSideChatId(page.chatId)),
  };
}
