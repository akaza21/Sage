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
import { agentDiffKey, type AgentDiffRef } from "../../../domain/agentDiff";
import { ChatFileChip } from "./FileChip";
import { chatFileChipBasename } from "./chatFileChip";
import { projectAgentEditSummary } from "./agentEditSummary";
import { InlineSavedDiff } from "./InlineSavedDiff";

interface TurnEditSummaryProps {
  page: AcpChatPage;
  turn: TurnProjection;
  rootLabels: Readonly<Record<string, string>>;
  countIncomplete: boolean;
  onOpenFile?: (resource: ResourceRef) => void;
  onOpenDiff?: (resource: ResourceRef, snapshot: AgentDiffRef) => void;
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
  onOpenFile,
  onOpenDiff,
  onRecordDisposition,
}: TurnEditSummaryProps) {
  const summary = projectAgentEditSummary(page, turn);
  const completed = turn.state === "completed";
  const [expansion, setExpansion] = useState({
    turnId: turn.id,
    completed,
    expanded: !completed,
  });
  const expanded =
    expansion.turnId === turn.id && expansion.completed === completed
      ? expansion.expanded
      : !completed;
  const [recording, setRecording] = useState<AgentEditDispositionState | null>(
    null,
  );
  const [dispositionError, setDispositionError] = useState(false);
  const [previewKey, setPreviewKey] = useState<string | null>(null);
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

  const noun = summary.changes.length === 1 ? "file" : "files";
  const fileLabel = summaryIncomplete
    ? `Showing ${summary.changes.length} changed ${noun}`
    : `${summary.changes.length} ${noun} changed`;
  const canCollapse = hasInteractiveChanges;

  return (
    <section
      className={`acp-file-changes${!expanded && canCollapse ? " acp-file-changes--collapsed" : ""}`}
      aria-label={`Files edited in turn ${turn.ordinal}`}
      data-session-attempt={turn.sessionId}
    >
      {canCollapse ? (
        <button
          className="acp-file-changes__header"
          type="button"
          aria-expanded={expanded}
          onClick={() => {
            setExpansion({ turnId: turn.id, completed, expanded: !expanded });
            if (expanded) setPreviewKey(null);
          }}
        >
          <Files size={16} aria-hidden="true" />
          <strong>{fileLabel}</strong>
          {dispositionError ? (
            <span className="acp-file-changes__error">Needs attention</span>
          ) : null}
          <span className="acp-file-changes__totals" aria-label="Line changes">
            <span>+{additions}</span>
            <span>-{deletions}</span>
          </span>
        </button>
      ) : (
        <header className="acp-file-changes__header">
          <Files size={16} aria-hidden="true" />
          <strong>{fileLabel}</strong>
          <span className="acp-file-changes__totals" aria-label="Line changes">
            <span>+{additions}</span>
            <span>-{deletions}</span>
          </span>
        </header>
      )}
      {(!canCollapse || expanded) && summary.changes.length > 0 ? (
        <ul>
          {summary.changes.map((change, index) => {
            const location = change.resource;
            if (location.kind === "unavailable") {
              const deletedDisplay =
                change.operation === "deleted" && location.reason === "missing"
                  ? location.displayResource?.relativePath
                  : null;
              return (
                <li key={`unavailable:${location.reason}:${index}`}>
                  <span className="acp-resource-unavailable">
                    {deletedDisplay
                      ? `${deletedDisplay} · Deleted`
                      : `${location.label} · unavailable`}
                  </span>
                  {deletedDisplay ? null : (
                    <span className="acp-change-operation">
                      {change.operation}
                    </span>
                  )}
                  <span className="acp-file-changes__counts">
                    <span>
                      {change.additions === null ? "—" : `+${change.additions}`}
                    </span>
                    <span>
                      {change.deletions === null ? "—" : `-${change.deletions}`}
                    </span>
                  </span>
                  <span className="acp-file-changes__summary">
                    {change.summary}
                  </span>
                </li>
              );
            }
            const resource = location.resource;
            const rootName = rootDisplayName(rootLabels, resource.rootId);
            const qualifiedLabel = `${rootName} / ${resource.relativePath}`;
            const admitted = chatFileChipBasename(resource.relativePath);
            const chip = admitted ? (
              <ChatFileChip
                path={resource.relativePath}
                rootLabel={rootName}
                additions={change.additions}
                deletions={change.deletions}
              />
            ) : (
              <span className="acp-resource-unavailable">unavailable</span>
            );
            const counted =
              change.additions !== null || change.deletions !== null;
            return (
              <li key={resourceKey(resource)}>
                {admitted &&
                ((onOpenDiff && change.diffRef) ||
                  (onOpenFile &&
                    !change.diffRef &&
                    change.operation !== "deleted")) ? (
                  <button
                    type="button"
                    className="acp-resource-action"
                    aria-label={`Open ${change.diffRef ? "diff" : "file"} ${qualifiedLabel}`}
                    onClick={() => {
                      if (change.diffRef)
                        onOpenDiff?.(resource, change.diffRef);
                      else onOpenFile?.(resource);
                    }}
                  >
                    {chip}
                    <ChevronRight size={14} aria-hidden="true" />
                  </button>
                ) : (
                  chip
                )}
                <span className="acp-change-operation">{change.operation}</span>
                {admitted && counted ? null : (
                  <span className="acp-file-changes__counts">
                    <span>
                      {change.additions === null ? "—" : `+${change.additions}`}
                    </span>
                    <span>
                      {change.deletions === null ? "—" : `-${change.deletions}`}
                    </span>
                  </span>
                )}
                <span className="acp-file-changes__summary">
                  {change.summary}
                </span>
                {admitted && change.diffRef ? (
                  <button
                    type="button"
                    className="acp-file-changes__preview-toggle"
                    aria-label={`Preview saved diff ${qualifiedLabel}`}
                    aria-expanded={previewKey === agentDiffKey(change.diffRef)}
                    onClick={() =>
                      setPreviewKey((current) =>
                        current === agentDiffKey(change.diffRef!)
                          ? null
                          : agentDiffKey(change.diffRef!),
                      )
                    }
                  >
                    Preview saved diff
                  </button>
                ) : null}
                {change.diffRef &&
                previewKey === agentDiffKey(change.diffRef) ? (
                  <InlineSavedDiff
                    key={previewKey}
                    resource={resource}
                    diffRef={change.diffRef}
                  />
                ) : null}
              </li>
            );
          })}
        </ul>
      ) : null}
      {(!canCollapse || expanded) &&
      hasInteractiveChanges &&
      onRecordDisposition ? (
        <div className="acp-file-changes__disposition">
          <button
            type="button"
            disabled={recording !== null}
            onClick={() => void recordDisposition("accepted")}
          >
            {recording === "accepted" ? "Recording…" : "Mark reviewed"}
          </button>
          <button
            type="button"
            disabled={recording !== null}
            onClick={() => void recordDisposition("rejected")}
          >
            {recording === "rejected" ? "Recording…" : "Needs follow-up"}
          </button>
          <span>
            This records review status only. It does not apply or revert files.
          </span>
        </div>
      ) : null}
      {(!canCollapse || expanded) && dispositionError ? (
        <p className="acp-file-changes__disclosure" role="alert">
          The edit disposition was not recorded. No workspace files were
          changed.
        </p>
      ) : null}
      {(!canCollapse || expanded) && summary.omittedCount > 0 ? (
        <p className="acp-file-changes__disclosure">
          {summary.omittedCount} additional changed files not shown.
        </p>
      ) : null}
      {(!canCollapse || expanded) && summary.sourceTruncated ? (
        <p className="acp-file-changes__disclosure">
          The agent reported further changed files that are not shown.
        </p>
      ) : null}
      {(!canCollapse || expanded) && summary.rejectedUnsafeOrStale ? (
        <p className="acp-file-changes__disclosure">
          Unsafe or stale change records were not shown.
        </p>
      ) : null}
    </section>
  );
}
