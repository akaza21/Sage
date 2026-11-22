import type { AcpChatPage, TurnProjection } from "../../domain/acp";
import { decodeSessionSurface } from "../../services/acpSessionSurface";
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
    undoTurn: decodeSessionSurface(page.sessionSurface)?.undoTurn,
  };
}

/** Only currently loaded, finished prompts after the newest compaction can act. */
export function conversationActions(
  page: AcpChatPage,
  turn: TurnProjection,
  state = conversationState(page),
) {
  return loadedTurnActions(
    page,
    turn,
    state,
    page.turns.some((row) => row.id === turn.id),
  );
}

/** One fold per page: the ids of loaded turns that offer rewind or fork. */
export function pageConversationActions(page: AcpChatPage) {
  const state = conversationState(page);
  const offered = new Set<string>();
  for (const turn of page.turns) {
    const actions = loadedTurnActions(page, turn, state, true);
    if (actions.rewind || actions.fork) offered.add(turn.id);
  }
  return offered;
}

function loadedTurnActions(
  page: AcpChatPage,
  turn: TurnProjection,
  state: ReturnType<typeof conversationState>,
  loaded: boolean,
) {
  const { offer, boundary, hot, sessionId, busy, latest, undoTurn } = state;
  const latestOnly = offer?.targets === "latestTurn";
  const available =
    offer &&
    (latestOnly ? undoTurn === turn.id : turn.conversationTarget === true) &&
    turn.ordinal > boundary &&
    loaded &&
    sessionId === turn.sessionId &&
    hot &&
    turn.terminalAt != null &&
    !busy &&
    (!latestOnly || turn.id === latest);
  return {
    rewind: !!(available && offer.effects.conversation),
    fork: !!(
      available &&
      !latestOnly &&
      offer.forkAt &&
      !isSideChatId(page.chatId)
    ),
  };
}
