import type { AcpChatPage, TurnProjection } from "../../../domain/acp";
import {
  MAX_AGENT_EDIT_SUMMARY_FILES,
  agentEditSummaryChangeKey,
  projectAgentEditSummary,
  type AgentEditSummaryChange,
  type AgentEditSummaryProjection,
} from "./agentEditSummary";
import { terminalItemLifecycles } from "./statusSemantics";
import type { TurnWorkspaceChange } from "./turnWorkspaceChanges";

const retainedTurns = new WeakMap<
  TurnProjection,
  {
    projectId: string;
    chatId: string;
    sessionKnown: boolean;
    summary: AgentEditSummaryProjection;
  }
>();

function workspaceChange(change: TurnWorkspaceChange): AgentEditSummaryChange {
  return {
    resource: {
      kind: "resource",
      resource: { rootId: change.rootId, relativePath: change.relativePath },
    },
    operation: change.operation,
    originalResource: null,
    summary: "",
    additions: null,
    deletions: null,
  };
}

function mergeWorkspaceChanges(
  summary: AgentEditSummaryProjection,
  turn: TurnProjection,
  workspaceChanges: readonly TurnWorkspaceChange[],
): AgentEditSummaryProjection {
  const changes = new Map<string, AgentEditSummaryChange>();
  const changeKeys = new Set(summary.changeKeys);
  for (const workspace of workspaceChanges) {
    if (workspace.turnId !== turn.id) continue;
    const change = workspaceChange(workspace);
    const key = agentEditSummaryChangeKey(change);
    if (summary.changeKeys.has(key)) continue;
    changeKeys.add(key);
    changes.delete(key);
    changes.set(key, change);
  }
  for (const change of summary.changes) {
    const key = agentEditSummaryChangeKey(change);
    changes.delete(key);
    changes.set(key, change);
  }
  while (changes.size > MAX_AGENT_EDIT_SUMMARY_FILES)
    changes.delete(changes.keys().next().value!);
  return {
    ...summary,
    changes: [...changes.values()],
    changeKeys,
    omittedCount: changeKeys.size - changes.size,
  };
}

export function projectTurnEditSummary(
  page: AcpChatPage,
  turn: TurnProjection,
  workspaceChanges: readonly TurnWorkspaceChange[] = [],
  workspaceIncomplete = false,
) {
  const summary = projectAgentEditSummary(page, turn);
  return mergeWorkspaceChanges(
    {
      ...summary,
      sourceTruncated: summary.sourceTruncated || workspaceIncomplete,
    },
    turn,
    workspaceChanges,
  );
}

export function projectChatEditSummary(
  page: AcpChatPage,
  workspaceChanges: readonly TurnWorkspaceChange[] = [],
  workspaceIncomplete = false,
) {
  const changeKeys = new Set<string>();
  const changes = new Map<
    string,
    AgentEditSummaryChange & { turn: TurnProjection }
  >();
  const summary: AgentEditSummaryProjection = {
    changes: [],
    changeKeys,
    omittedCount: 0,
    sourceTruncated: workspaceIncomplete,
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
    const merged = mergeWorkspaceChanges(retained, turn, workspaceChanges);
    summary.sourceTruncated ||= merged.sourceTruncated;
    summary.rejectedUnsafeOrStale ||= merged.rejectedUnsafeOrStale;
    for (const key of merged.changeKeys) {
      changeKeys.add(key);
      changes.delete(key);
    }
    for (const change of merged.changes) {
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
