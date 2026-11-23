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
          announce={false}
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
