import { Info } from "../../ui/leanIcons";

/** Load-earlier control and the notices about history that is not shown. */
export function AcpHistoryNotices({
  hasEarlier,
  loadingEarlier,
  historyError,
  onLoadEarlier,
  unloaded,
  retentionIncomplete,
}: {
  hasEarlier: boolean;
  loadingEarlier: boolean;
  historyError: string | null;
  onLoadEarlier?: () => void;
  /** Earlier pages exist that have not been loaded. */
  unloaded: boolean;
  retentionIncomplete: boolean;
}) {
  return (
    <>
      {hasEarlier && onLoadEarlier ? (
        <button
          type="button"
          className="acp-history-action"
          disabled={loadingEarlier}
          onClick={onLoadEarlier}
        >
          {loadingEarlier ? "Loading earlier history…" : "Load earlier history"}
        </button>
      ) : null}
      {historyError ? (
        <p className="acp-history-error" role="alert" aria-live="off">
          Earlier history could not be loaded: {historyError}
        </p>
      ) : null}
      {unloaded ? (
        <p className="acp-history-notice">
          Earlier history has not been loaded. Changed-file summaries show
          retained records only.
        </p>
      ) : null}
      {retentionIncomplete ? (
        <p className="acp-history-notice">
          Some Chat history was discarded to stay within memory limits.
          Changed-file summaries show retained records only.
        </p>
      ) : null}
    </>
  );
}

/** A system note about the turn, styled apart from the agent's own prose. */
export function WorkspaceTrackingNotice() {
  return (
    <p
      className="acp-system-notice"
      role="status"
      aria-label="Workspace tracking"
    >
      <Info size={14} aria-hidden="true" />
      <span className="sr-only">System: </span>
      <span>Workspace change tracking is incomplete for this turn.</span>
    </p>
  );
}
