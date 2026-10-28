import type { AcpChatPage } from "../../domain/acpProjection";
import type { AgentDiffRef } from "../../domain/agentDiff";
import type {
  ProjectRepositoryStatus,
  ResourceRef,
} from "../../domain/workbench";
import { projectAgentEditSummary } from "../chat/acp/agentEditSummary";

export type ChangesScope = "lastTurn" | "unstaged" | "staged";
export type ChangesEntry = {
  resource: ResourceRef;
  additions: number | null;
  deletions: number | null;
  diffRef?: AgentDiffRef;
  unavailable?: string;
};

export function projectChanges(
  scope: ChangesScope,
  page: AcpChatPage | null,
  repositories: ProjectRepositoryStatus,
) {
  const entries: ChangesEntry[] = [];
  if (scope === "lastTurn") {
    let turn;
    for (
      let index = (page?.turns.length ?? 0) - 1;
      index >= Math.max(0, (page?.turns.length ?? 0) - 128);
      index -= 1
    ) {
      const candidate = page!.turns[index];
      if (candidate.sessionId === page!.currentSessionId) {
        turn = candidate;
        break;
      }
    }
    if (!page || !turn) return { entries, truncated: false };
    const summary = projectAgentEditSummary(page, turn);
    for (const change of summary.changes) {
      if (change.resource.kind !== "resource") continue;
      entries.push({
        resource: change.resource.resource,
        additions: change.additions,
        deletions: change.deletions,
        diffRef: change.diffRef,
        ...(!change.diffRef
          ? {
              unavailable:
                "The harness did not retain a before/after pair for this file.",
            }
          : {}),
      });
    }
    return {
      entries,
      truncated:
        summary.sourceTruncated ||
        summary.omittedCount > 0 ||
        summary.rejectedUnsafeOrStale,
    };
  }
  let truncated = repositories.truncated || repositories.roots.length > 32;
  for (const root of repositories.roots.slice(0, 32)) {
    truncated ||= root.truncated || root.changes.length > 300;
    if (root.state !== "repository") continue;
    for (const change of root.changes.slice(0, 300)) {
      const indexed = change.indexStatus !== "unchanged";
      const working = change.worktreeStatus !== "unchanged";
      if (!(scope === "staged" ? indexed : working)) continue;
      if (entries.length === 300) return { entries, truncated: true };
      const unavailable =
        indexed && working
          ? "Separate diffs for files with both staged and unstaged changes are not exposed by the Git backend."
          : change.indexStatus === "conflicted" ||
              change.worktreeStatus === "conflicted"
            ? "Resolve this file's merge conflict before viewing its scoped diff."
            : undefined;
      entries.push({
        resource: change.resource,
        additions: unavailable ? null : change.additions,
        deletions: unavailable ? null : change.deletions,
        unavailable,
      });
    }
  }
  return { entries, truncated };
}
