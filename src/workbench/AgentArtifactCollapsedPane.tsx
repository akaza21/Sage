import { PanelRightOpen } from "../ui/leanIcons";
import type { RefObject } from "react";

export function AgentArtifactCollapsedPane({
  count,
  edgeTriggerRef,
  onExpand,
  paneRef,
}: {
  count: number;
  edgeTriggerRef: RefObject<HTMLButtonElement | null>;
  onExpand: () => void;
  paneRef: RefObject<HTMLElement | null>;
}) {
  const label =
    count > 0
      ? `Show preview · Ready · ${count} ${count === 1 ? "resource" : "resources"}`
      : "Show preview · Waiting";
  return (
    <aside
      ref={paneRef}
      className="agent-artifact-pane agent-artifact-pane--collapsed"
      aria-label="Agent artifacts"
    >
      <button
        ref={edgeTriggerRef}
        className="agent-artifact-pane__edge-action"
        type="button"
        aria-label={label}
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
    </aside>
  );
}
