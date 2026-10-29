import { memo, useMemo, useState } from "react";
import { Files, ChevronRight } from "../../../ui/leanIcons";

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
import { projectChatEditSummary } from "./chatEditSummary";
import { DeferredInlineSavedDiff as InlineSavedDiff } from "./DeferredChatDetails";
import { FileChangeTotals } from "./FileChangeTotals";
import { isTauriRuntime } from "../../../ports/desktopRuntime";
import type { TurnWorkspaceChange } from "./turnWorkspaceChanges";
import { useTurnWorkspaceChanges } from "./turnWorkspaceBridge";

interface TurnEditSummaryProps {
  page: AcpChatPage;
  rootLabels: Readonly<Record<string, string>>;
  countIncomplete: boolean;
  workspaceChanges?: readonly TurnWorkspaceChange[];
  onOpenFile?: (resource: ResourceRef, turn: TurnProjection) => void;
  onOpenDiff?: (resource: ResourceRef, snapshot: AgentDiffRef) => void;
  onRecordDisposition?: (
    disposition: AgentEditDisposition,
  ) => Promise<AcpMutationResult>;
}

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

export const TurnEditSummary = memo(function TurnEditSummary({
  page,
  rootLabels,
  countIncomplete,
  workspaceChanges,
  onOpenFile,
  onOpenDiff,
  onRecordDisposition,
}: TurnEditSummaryProps) {
  const fetchedWorkspaceChanges = useTurnWorkspaceChanges(
    page,
    workspaceChanges === undefined && isTauriRuntime(),
  );
  const workspace = workspaceChanges ?? fetchedWorkspaceChanges;
  const summary = useMemo(
    () => projectChatEditSummary(page, workspace),
    [page, workspace],
  );
  const [expanded, setExpanded] = useState(false);
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
  const hasInteractiveChanges = summary.changes.some(
    (change) => change.resource.kind === "resource",
  );

  async function recordDisposition(
    state: AgentEditDispositionState,
    turn: TurnProjection,
  ) {
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
  const reviewTurns = [
    ...new Set(
      summary.changes
        .filter((change) => change.resource.kind === "resource")
        .map((change) => change.turn),
    ),
  ].filter((turn) => turn.sessionId === page.currentSessionId);

  return (
    <section
      className={`acp-file-changes${!expanded ? " acp-file-changes--collapsed" : ""}`}
      aria-label="Files edited in this chat"
      data-chat-id={page.chatId}
    >
      <button
        className="acp-file-changes__header"
        type="button"
        aria-expanded={expanded}
        onClick={() => {
          setExpanded(!expanded);
          if (expanded) setPreviewKey(null);
        }}
      >
        <Files size={16} aria-hidden="true" />
        <strong>{fileLabel}</strong>
        {dispositionError ? (
          <span className="acp-file-changes__error">Needs attention</span>
        ) : null}
        <FileChangeTotals changes={summary.changes} />
      </button>
      {expanded && summary.changes.length > 0 ? (
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
                change.turn.sessionId === page.currentSessionId &&
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
                      else onOpenFile?.(resource, change.turn);
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
                {admitted &&
                change.diffRef &&
                change.turn.sessionId === page.currentSessionId ? (
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
      {expanded && hasInteractiveChanges && onRecordDisposition
        ? reviewTurns.map((turn) => (
            <div
              key={turn.id}
              className="acp-file-changes__disposition"
              aria-label={"Review turn " + turn.ordinal}
            >
              {reviewTurns.length > 1 ? <span>Turn {turn.ordinal}</span> : null}
              <button
                type="button"
                disabled={recording !== null}
                onClick={() => void recordDisposition("accepted", turn)}
              >
                {recording === "accepted" ? "Recording…" : "Mark reviewed"}
              </button>
              <button
                type="button"
                disabled={recording !== null}
                onClick={() => void recordDisposition("rejected", turn)}
              >
                {recording === "rejected" ? "Recording…" : "Needs follow-up"}
              </button>
              <span>
                This records review status only. It does not apply or revert
                files.
              </span>
            </div>
          ))
        : null}
      {expanded && dispositionError ? (
        <p className="acp-file-changes__disclosure" role="alert">
          The edit disposition was not recorded. No workspace files were
          changed.
        </p>
      ) : null}
      {expanded && summary.omittedCount > 0 ? (
        <p className="acp-file-changes__disclosure">
          {summary.omittedCount} additional changed files not shown.
        </p>
      ) : null}
      {expanded && summary.sourceTruncated ? (
        <p className="acp-file-changes__disclosure">
          The agent reported further changed files that are not shown.
        </p>
      ) : null}
      {expanded && summary.rejectedUnsafeOrStale ? (
        <p className="acp-file-changes__disclosure">
          Unsafe or stale change records were not shown.
        </p>
      ) : null}
    </section>
  );
});
