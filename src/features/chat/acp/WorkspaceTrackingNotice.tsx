export function WorkspaceTrackingNotice() {
  return (
    <p
      className="acp-history-notice"
      role="status"
      aria-label="Workspace tracking"
    >
      <span className="sr-only">System: </span>
      Workspace change tracking is incomplete for this turn.
    </p>
  );
}
