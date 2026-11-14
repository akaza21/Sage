import type { AgentDiffRef } from "./agentDiff";
import type { WorkbenchTab } from "./chat";

export const AGENT_DIFF_SIDE_BYTES = 262_144;
export const MAX_AGENT_DIFFS_PER_TURN = 32;

/** Preserve the snapshot variant even when damaged/older restore loses its ref. */
export function isSnapshotDiffTab(
  tab: Pick<WorkbenchTab, "id" | "kind" | "diffRef">,
): boolean {
  return (
    tab.kind === "diff" &&
    Boolean(tab.diffRef || tab.id.startsWith("diff:snapshot:"))
  );
}

export function agentDiffKey(ref: AgentDiffRef): string {
  return `snapshot:${JSON.stringify([
    ref.projectId,
    ref.chatId,
    ref.sessionId,
    ref.runtimeEpoch,
    ref.turnId,
    ref.itemId,
    ref.diffOrdinal,
    ref.snapshotRevision,
    ref.rootId,
    ref.relativePath,
    ref.contentDigest,
  ])}`;
}
