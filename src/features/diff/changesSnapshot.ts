import type { AcpChatPage } from "../../domain/acp";
import type { TurnWorkspaceLedger } from "../chat/acp/turnWorkspaceChanges";
import {
  mergeWorkspaceChanges,
  projectTurnEditSummary,
} from "../chat/acp/chatEditSummary";
import {
  lastChangesTurn,
  projectSummaryChanges,
  type ChangesEntry,
} from "./agentChanges";

/** Changes need one turn's edit metadata, never its transcript or activities. */
export function changesSnapshot(page: AcpChatPage | null) {
  if (!page) return null;
  const turn = lastChangesTurn(page);
  const ownerPage: AcpChatPage = {
    projectId: page.projectId,
    chatId: page.chatId,
    title: "",
    projectionRevision: page.projectionRevision,
    currentSessionId: page.currentSessionId,
    sessions: page.sessions.filter((session) => session.id === turn?.sessionId),
    turns: turn
      ? [
          {
            id: turn.id,
            sessionId: turn.sessionId,
            runtimeEpoch: turn.runtimeEpoch,
            ordinal: turn.ordinal,
            state: turn.state,
            revision: turn.revision,
            createdAt: turn.createdAt,
            updatedAt: turn.updatedAt,
            terminalAt: turn.terminalAt,
            items: [],
            activities: [],
            browserOperations: [],
            browserOperationsCollapsed: 0,
          },
        ]
      : [],
    nextCursor: null,
  };
  const summary = turn ? projectTurnEditSummary(page, turn) : null;
  return {
    page: ownerPage,
    summary: summary
      ? {
          ...summary,
          changes: summary.changes.map((change) => ({
            ...change,
            summary: "",
          })),
        }
      : null,
  };
}

/** Merge native shell edits with the retained, validated file-change summary. */
export function snapshotChanges(
  snapshot: ReturnType<typeof changesSnapshot>,
  ledger: TurnWorkspaceLedger,
) {
  const turn = snapshot?.page.turns[0];
  if (!snapshot?.summary || !turn)
    return { entries: [] as ChangesEntry[], truncated: false };
  return projectSummaryChanges(
    mergeWorkspaceChanges(
      {
        ...snapshot.summary,
        sourceTruncated:
          snapshot.summary.sourceTruncated ||
          ledger.incompleteTurnIds.has(turn.id),
      },
      turn,
      ledger.changes,
    ),
  );
}
