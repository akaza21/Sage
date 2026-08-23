import { useState } from "react";
import { ChevronRight, Files } from "lucide-react";

import type {
  AcpChatPage,
  AcpMutationResult,
  AgentEditDisposition,
  AgentEditDispositionState,
  TurnProjection,
} from "../../../domain/acp";
import { resourceKey } from "../../../domain/resourceIdentity";
import type { ResourceRef } from "../../../domain/workbench";
import { projectAgentEditSummary } from "./agentEditSummary";

interface TurnEditSummaryProps {
  page: AcpChatPage;
  turn: TurnProjection;
  rootLabels: Readonly<Record<string, string>>;
  countIncomplete: boolean;
  onOpenDiff?: (resource: ResourceRef) => void;
  onRecordDisposition?: (
    disposition: AgentEditDisposition,
  ) => Promise<AcpMutationResult>;
}

const MAX_FILE_CHANGE_LINES = 1_000_000;

function rootDisplayName(
  rootLabels: Readonly<Record<string, string>>,
  rootId: string,
): string {
  const projected = Object.prototype.hasOwnProperty.call(rootLabels, rootId)
    ? rootLabels[rootId]
    : null;
  return typeof projected === "string" && projected.length > 0
    ? projected
    : rootId;
}

export function TurnEditSummary({
  page,
  turn,
  rootLabels,
  countIncomplete,
  onOpenDiff,
  onRecordDisposition,
}: TurnEditSummaryProps) {
  const summary = projectAgentEditSummary(page, turn);
  const [recording, setRecording] = useState<AgentEditDispositionState | null>(
    null,
  );
  const [dispositionError, setDispositionError] = useState(false);
  if (
    summary.changes.length === 0 &&
    !summary.sourceTruncated &&
    !summary.rejectedUnsafeOrStale
  ) {
    return null;
  }
  const summaryIncomplete =
    countIncomplete ||
    summary.omittedCount > 0 ||
    summary.sourceTruncated ||
    summary.rejectedUnsafeOrStale;
  const additions = summary.changes.reduce(
    (total, change) =>
      Math.min(MAX_FILE_CHANGE_LINES, total + (change.additions ?? 0)),
    0,
  );
  const deletions = summary.changes.reduce(
    (total, change) =>
      Math.min(MAX_FILE_CHANGE_LINES, total + (change.deletions ?? 0)),
    0,
  );
  const hasInteractiveChanges = summary.changes.some(
    (change) => change.resource.kind === "resource",
  );

  async function recordDisposition(state: AgentEditDispositionState) {
    if (!onRecordDisposition || recording) return;
    setRecording(state);
    setDispositionError(false);
    try {
      await onRecordDisposition({
        projectId: page.projectId,
        chatId: page.chatId,
        sessionId: turn.sessionId,
        turnId: turn.id,
        expectedRevision: turn.revision,
        state,
      });
    } catch {
      setDispositionError(true);
    } finally {
      setRecording(null);
    }
  }

  return (
    <section
      className="acp-file-changes"
      aria-label={`Files edited in turn ${turn.ordinal}`}
      data-session-attempt={turn.sessionId}
    >
      <header className="acp-file-changes__header">
        <Files size={16} aria-hidden="true" />
        <strong>
          {summaryIncomplete
            ? `Showing ${summary.changes.length} edited ${summary.changes.length === 1 ? "file" : "files"}`
            : `Edited ${summary.changes.length} ${summary.changes.length === 1 ? "file" : "files"}`}
        </strong>
        <span className="acp-file-changes__totals" aria-label="Line changes">
          <span>+{additions}</span>
          <span>-{deletions}</span>
        </span>
      </header>
      {summary.changes.length > 0 ? (
        <ul>
          {summary.changes.map((change, index) => {
            const location = change.resource;
            if (location.kind === "unavailable") {
              return (
                <li key={`unavailable:${location.reason}:${index}`}>
                  <span className="acp-resource-unavailable">
                    {location.label} · unavailable
                  </span>
                  <span className="acp-change-operation">
                    {change.operation}
                  </span>
                  <span className="acp-file-changes__counts">
                    <span>
                      {change.additions === null ? "—" : `+${change.additions}`}
                    </span>
                    <span>
                      {change.deletions === null ? "—" : `-${change.deletions}`}
                    </span>
                  </span>
                  <span>{change.summary}</span>
                </li>
              );
            }
            const resource = location.resource;
            const rootName = rootDisplayName(rootLabels, resource.rootId);
            const qualifiedLabel = `${rootName} / ${resource.relativePath}`;
            return (
              <li key={resourceKey(resource)}>
                {onOpenDiff ? (
                  <button
                    type="button"
                    className="acp-resource-action"
                    aria-label={`Open diff ${qualifiedLabel}`}
                    onClick={() => onOpenDiff(resource)}
                  >
                    <span className="acp-resource-root">{rootName}</span>
                    <span>{resource.relativePath}</span>
                    <ChevronRight size={14} aria-hidden="true" />
                  </button>
                ) : (
                  <span>{qualifiedLabel}</span>
                )}
                <span className="acp-change-operation">{change.operation}</span>
                <span className="acp-file-changes__counts">
                  <span>
                    {change.additions === null ? "—" : `+${change.additions}`}
                  </span>
                  <span>
                    {change.deletions === null ? "—" : `-${change.deletions}`}
                  </span>
                </span>
                <span>{change.summary}</span>
              </li>
            );
          })}
        </ul>
      ) : null}
      {hasInteractiveChanges && onRecordDisposition ? (
        <div className="acp-file-changes__disposition">
          <button
            type="button"
            disabled={recording !== null}
            onClick={() => void recordDisposition("accepted")}
          >
            {recording === "accepted" ? "Accepting…" : "Accept"}
          </button>
          <button
            type="button"
            disabled={recording !== null}
            onClick={() => void recordDisposition("rejected")}
          >
            {recording === "rejected" ? "Discarding…" : "Discard"}
          </button>
        </div>
      ) : null}
      {dispositionError ? (
        <p className="acp-file-changes__disclosure" role="alert">
          The edit disposition was not recorded. No workspace files were
          changed.
        </p>
      ) : null}
      {summary.omittedCount > 0 ? (
        <p className="acp-file-changes__disclosure">
          {summary.omittedCount} additional changed files not shown.
        </p>
      ) : null}
      {summary.sourceTruncated ? (
        <p className="acp-file-changes__disclosure">
          The agent reported further changed files that are not shown.
        </p>
      ) : null}
      {summary.rejectedUnsafeOrStale ? (
        <p className="acp-file-changes__disclosure">
          Unsafe or stale change records were not shown.
        </p>
      ) : null}
    </section>
  );
}
