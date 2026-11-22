import { Info } from "../../ui/leanIcons";

export function WorkspaceTrackingNotice() {
  return (
    <p
      className="acp-system-notice"
      role="status"
      aria-label="Workspace tracking"
    >
      <Info size={14} />
      <span className="sr-only">System: </span>
      Workspace change tracking is incomplete for this turn.
    </p>
  );
}
