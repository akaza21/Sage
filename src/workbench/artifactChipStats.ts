import type {
  AcpChatPage,
  FileChangeProjection,
  TurnProjection,
} from "../domain/acp";
import type { TabKind } from "../domain/chat";
import { resourceKey } from "../domain/resourceIdentity";
import type { ResourceRef } from "../domain/workbench";
import { projectAgentEditSummary } from "../features/chat/acp/agentEditSummary";
import { terminalTurnStates } from "../features/chat/acp/statusSemantics";

export const ARTIFACT_CHIP_COUNT_CAP = 999;

export type ArtifactChipTab = {
  kind: TabKind | string;
  resource?: ResourceRef;
};

export type ArtifactChipStats = {
  additionsLabel: string | null;
  deletionsLabel: string | null;
};

function currentOrLatestTurn(page: AcpChatPage): TurnProjection | null {
  if (!page.currentSessionId) return null;
  let current: TurnProjection | null = null;
  let latest: TurnProjection | null = null;
  for (const turn of page.turns) {
    if (turn.sessionId !== page.currentSessionId) continue;
    if (!latest || turn.ordinal > latest.ordinal) latest = turn;
    if (
      !terminalTurnStates.has(turn.state) &&
      (!current || turn.ordinal > current.ordinal)
    ) {
      current = turn;
    }
  }
  return current ?? latest;
}

function formatChipCount(sign: "+" | "-", value: number | null): string | null {
  if (value === null) return null;
  if (value > ARTIFACT_CHIP_COUNT_CAP)
    return `${sign}${ARTIFACT_CHIP_COUNT_CAP}+`;
  return `${sign}${value}`;
}

function changeMatchesTab(
  change: FileChangeProjection,
  resource: ResourceRef,
): boolean {
  if (change.resource.kind !== "resource") return false;
  return resourceKey(change.resource.resource) === resourceKey(resource);
}

export function artifactChipStatsForTab(
  tab: ArtifactChipTab,
  page: AcpChatPage | null,
): ArtifactChipStats | null {
  const resource = tab.resource;
  if ((tab.kind !== "file" && tab.kind !== "diff") || !resource || !page) {
    return null;
  }
  const turn = currentOrLatestTurn(page);
  if (!turn) return null;
  const summary = projectAgentEditSummary(page, turn);
  const matched = summary.changes.find((change) =>
    changeMatchesTab(change, resource),
  );
  if (!matched) return null;
  const additionsLabel = formatChipCount("+", matched.additions);
  const deletionsLabel = formatChipCount("-", matched.deletions);
  if (additionsLabel === null && deletionsLabel === null) return null;
  return { additionsLabel, deletionsLabel };
}
