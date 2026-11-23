import type { AgentDiffRef } from "./agentDiff";
import type { WorkbenchTab } from "./chat";

export const AGENT_DIFF_SIDE_BYTES = 262_144;

/** Diff load reasons Sage itself writes; any other failure text is a diagnostic. */
export const SAVED_EDIT_UNAVAILABLE =
  "This saved edit is unavailable for the current chat session.";
export const CHAT_CHANGED_WHILE_LOADING =
  "The chat changed while loading this edit. Reopen the diff to retry.";
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
