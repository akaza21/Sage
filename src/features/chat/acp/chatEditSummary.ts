import type { AcpChatPage, TurnProjection } from "../../../domain/acp";
import {
  MAX_AGENT_EDIT_SUMMARY_FILES,
  agentEditSummaryChangeKey,
  projectAgentEditSummary,
  type AgentEditSummaryChange,
  type AgentEditSummaryProjection,
} from "./agentEditSummary";
import { terminalItemLifecycles } from "./statusSemantics";

const retainedTurns = new WeakMap<
  TurnProjection,
  {
    projectId: string;
    chatId: string;
    sessionKnown: boolean;
    summary: AgentEditSummaryProjection;
  }
>();

export function projectChatEditSummary(page: AcpChatPage) {
  const changeKeys = new Set<string>();
  const changes = new Map<
    string,
    AgentEditSummaryChange & { turn: TurnProjection }
  >();
  const summary: AgentEditSummaryProjection = {
    changes: [],
    changeKeys,
    omittedCount: 0,
    sourceTruncated: false,
    rejectedUnsafeOrStale: false,
  };
  for (const turn of page.turns) {
    const sessionKnown = page.sessions.some(
      (session) => session.id === turn.sessionId,
    );
    const cached = retainedTurns.get(turn);
    const retained =
      cached?.projectId === page.projectId &&
      cached.chatId === page.chatId &&
      cached.sessionKnown === sessionKnown
        ? cached.summary
        : projectAgentEditSummary(page, {
            ...turn,
            items: turn.items.filter((item) =>
              terminalItemLifecycles.has(item.lifecycle),
            ),
          });
    if (retained !== cached?.summary)
      retainedTurns.set(turn, {
        projectId: page.projectId,
        chatId: page.chatId,
        sessionKnown,
        summary: retained,
      });
    summary.sourceTruncated ||= retained.sourceTruncated;
    summary.rejectedUnsafeOrStale ||= retained.rejectedUnsafeOrStale;
    for (const key of retained.changeKeys) {
      changeKeys.add(key);
      changes.delete(key);
    }
    for (const change of retained.changes) {
      const key = agentEditSummaryChangeKey(change);
      changes.delete(key);
      changes.set(key, {
        ...change,
        turn,
      });
      if (changes.size > MAX_AGENT_EDIT_SUMMARY_FILES)
        changes.delete(changes.keys().next().value!);
    }
  }
  summary.omittedCount = changeKeys.size - changes.size;
  return {
    ...summary,
    changes: [...changes.values()],
  };
}
