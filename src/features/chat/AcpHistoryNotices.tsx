import { Info } from "../../ui/leanIcons";
import { Notice } from "../../ui/Notice";

export function AcpHistoryNotices({
  hasEarlier,
  loadingEarlier,
  historyError,
  onLoadEarlier,
  unloaded,
  retentionIncomplete,
  scope = "current-chat",
}: {
  hasEarlier: boolean;
  loadingEarlier: boolean;
  historyError: string | null;
  onLoadEarlier?: () => void;
  unloaded: boolean;
  retentionIncomplete: boolean;
  scope?: string;
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
        <Notice
          identity={`history-read:${scope}`}
          severity="warning"
          message="Earlier history could not be loaded."
          details={historyError}
        />
      ) : null}
      {unloaded || retentionIncomplete ? (
        <Notice
          identity={`history-boundary:${scope}`}
          severity="info"
          message={`${
            unloaded
              ? retentionIncomplete
                ? "Earlier history has not been loaded and some history was discarded."
                : "Earlier history has not been loaded."
              : "Some chat history was discarded."
          } Changed-file summaries show retained records only.`}
          details={
            retentionIncomplete
              ? "Older records were discarded to stay within memory limits."
              : undefined
          }
        />
      ) : null}
    </>
  );
}

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
