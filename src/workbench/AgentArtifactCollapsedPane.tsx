import { PanelRightOpen } from "../ui/leanIcons";
import { useMemo, type RefObject } from "react";
import { createPortal } from "react-dom";
import type { AcpChatPage } from "../domain/acp";
import { projectTaskResources } from "./agentArtifactTabPresentation";

export function AgentArtifactCollapsedPane({
  count: artifactCount,
  subagentPage,
  subagentItemId,
  edgeTriggerRef,
  onExpand,
  paneId,
  overlay,
  headerNode,
}: {
  count: number;
  subagentPage?: AcpChatPage | null;
  subagentItemId?: string;
  edgeTriggerRef: RefObject<HTMLButtonElement | null>;
  onExpand: () => void;
  paneId: string;
  overlay: boolean;
  headerNode?: HTMLElement | null;
}) {
  // The header rule: each spawn invocation is one subagent, even when a
  // child thread id is reused; repeated observations of one spawn share it.
  const subagents = useMemo(() => {
    const turn = subagentPage?.turns.find((turn) =>
      turn.items.some((item) => item.id === subagentItemId),
    );
    return subagentPage && turn
      ? projectTaskResources(
          { ...subagentPage, turns: [turn] },
          subagentPage.chatId,
        ).recentAgents.length
      : 0;
  }, [subagentPage, subagentItemId]);
  const count = artifactCount + subagents;
  const label =
    count > 0
      ? `Show files and preview · Ready · ${count} ${count === 1 ? "resource" : "resources"}`
      : "Show files and preview · Waiting";
  const button = (
    <button
      ref={edgeTriggerRef}
      className="agent-artifact-pane__edge-action"
      type="button"
      aria-label={label}
      aria-controls={paneId}
      aria-haspopup={overlay ? "dialog" : undefined}
      aria-expanded={false}
      title={label}
      onClick={onExpand}
    >
      <PanelRightOpen size={17} strokeWidth={1.8} aria-hidden="true" />
      {count > 0 ? (
        <span className="agent-artifact-pane__count" aria-hidden="true">
          {count}
        </span>
      ) : (
        <span className="agent-artifact-pane__edge-dot" aria-hidden="true" />
      )}
    </button>
  );
  return headerNode ? createPortal(button, headerNode) : null;
}
