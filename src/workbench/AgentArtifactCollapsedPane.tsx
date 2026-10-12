import { PanelRightOpen } from "../ui/leanIcons";
import type { RefObject } from "react";
import { createPortal } from "react-dom";

export function AgentArtifactCollapsedPane({
  count,
  edgeTriggerRef,
  onExpand,
  paneId,
  overlay,
  headerNode,
}: {
  count: number;
  edgeTriggerRef: RefObject<HTMLButtonElement | null>;
  onExpand: () => void;
  paneId: string;
  overlay: boolean;
  headerNode?: HTMLElement | null;
}) {
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
