import type { AcpChatPage, TurnProjection } from "../../domain/acp";
import { isSideChatId } from "../../workbench/sideChatState";

// Two visible chats may share this lazy chunk. Cache at most two immutable
// page summaries; weak keys retain neither transcripts nor inactive views.
let summaries = new WeakMap<AcpChatPage, ReturnType<typeof summarize>>();
let summaryCount = 0;

export function conversationState(page: AcpChatPage) {
  const cached = summaries.get(page);
  if (cached) return cached;
  if (summaryCount === 2) {
    summaries = new WeakMap();
    summaryCount = 0;
  }
  const state = summarize(page);
  summaries.set(page, state);
  summaryCount++;
  return state;
}

/** Fold once per page, including when memoized older turns keep their props. */
function summarize(page: AcpChatPage) {
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
