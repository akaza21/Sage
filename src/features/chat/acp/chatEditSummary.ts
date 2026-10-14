import type { AcpChatPage, TurnProjection } from "../../../domain/acp";
import { resourceKey } from "../../../domain/resourceIdentity";
import {
  MAX_AGENT_EDIT_SUMMARY_FILES,
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

function sum(total: number | null | undefined, delta: number | null) {
  return total === null || delta === null ? null : (total ?? 0) + delta;
}

export function projectChatEditSummary(page: AcpChatPage) {
  const changes = new Map<
    string,
    AgentEditSummaryChange & { turn: TurnProjection }
  >();
  const summary: AgentEditSummaryProjection = {
    changes: [],
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
    summary.omittedCount += retained.omittedCount;
    summary.rejectedUnsafeOrStale ||= retained.rejectedUnsafeOrStale;
    const turnChanges = new Map<string, AgentEditSummaryChange>();
    for (const change of retained.changes) {
      const location = change.resource;
      const resource =
        location.kind === "resource"
          ? location.resource
          : location.displayResource;
      const key = resource ? resourceKey(resource) : JSON.stringify(location);
      turnChanges.delete(key);
      turnChanges.set(key, change);
    }
    for (const [key, change] of turnChanges) {
      const previous = changes.get(key);
      changes.delete(key);
      changes.set(key, {
        ...change,
        additions: sum(previous?.additions, change.additions),
        deletions: sum(previous?.deletions, change.deletions),
        turn,
      });
    }
  }
  summary.omittedCount += Math.max(
    0,
    changes.size - MAX_AGENT_EDIT_SUMMARY_FILES,
  );
  return {
    ...summary,
    changes: [...changes.values()].slice(-MAX_AGENT_EDIT_SUMMARY_FILES),
  };
}
