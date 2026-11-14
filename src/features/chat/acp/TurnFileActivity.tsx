import { useMemo, useState } from "react";
import type { AcpChatPage, TurnProjection } from "../../../domain/acp";
import type { AgentArtifactOpenHandler } from "../../../domain/agentArtifact";
import { ChevronDown, FilePenLine } from "../../../ui/leanIcons";
import { projectTurnEditSummary } from "./chatEditSummary";
import "./edited-files-card.css";
import { FileChangeTotals } from "./FileChangeTotals";
import { rootDisplayName } from "./rootDisplayName";
import type { TurnWorkspaceChange } from "./turnWorkspaceChanges";
import { editDiffAction } from "./chatEditSummary";
import { WorkspaceTrackingNotice } from "../AcpHistoryNotices";

export function TurnFileActivity({
  page,
  turn,
  rootLabels = {},
  workspaceChanges,
  workspaceIncomplete = false,
  onOpenResource,
}: {
  page: AcpChatPage;
  turn: TurnProjection;
  rootLabels?: Readonly<Record<string, string>>;
  workspaceChanges?: readonly TurnWorkspaceChange[];
  workspaceIncomplete?: boolean;
  onOpenResource?: AgentArtifactOpenHandler;
}) {
  const [expanded, setExpanded] = useState(false);
  const {
    changes,
    changeKeys,
    sourceTruncated,
    rejectedUnsafeOrStale,
    omittedCount,
  } = useMemo(
    () =>
      projectTurnEditSummary(page, turn, workspaceChanges, workspaceIncomplete),
    [page, turn, workspaceChanges, workspaceIncomplete],
  );
  if (!changes.length && !workspaceIncomplete) return null;
  const notice = workspaceIncomplete ? <WorkspaceTrackingNotice /> : null;
  // With no known file, an incomplete ledger is reported on its own rather
  // than as an "Edited at least 0 files" card with dead actions.
  if (!changes.length) return notice;
  const incomplete = sourceTruncated || rejectedUnsafeOrStale;
  const count = changeKeys.size;
  const open =
    turn.sessionId === page.currentSessionId ? onOpenResource : undefined;
  const firstDiff =
    changes.find((change) => change.diffRef) ??
    changes.find((change) => editDiffAction(change).resource);
  const visible = expanded ? changes : changes.slice(0, 3);
  const unavailable = !open
    ? "Changes cannot be opened from this chat session."
    : "No file baseline is available.";
  function view(change: (typeof changes)[number]) {
    const { resource } = editDiffAction(change);
    if (!open || !resource) return;
    open(
      resource,
      change.diffRef ? "diff" : "workingTreeDiff",
      {
        projectId: page.projectId,
        chatId: page.chatId,
        sessionId: turn.sessionId,
      },
      change.diffRef,
    );
  }
  return (
    <section className="acp-edited-files" aria-label="Edited files">
      {notice}
      <header className="acp-edited-files__header">
        <FilePenLine size={16} aria-hidden="true" />
        <div className="acp-edited-files__heading">
          <strong>
            Edited {incomplete ? "at least " : ""}
            {count} {count === 1 ? "file" : "files"}
          </strong>
          <FileChangeTotals
            changes={changes}
            incomplete={incomplete || omittedCount > 0}
          />
        </div>
        <div className="acp-edited-files__actions">
          <button
            type="button"
            disabled={!open || !firstDiff}
            title={
              !open || !firstDiff
                ? unavailable
                : editDiffAction(firstDiff).title
            }
            onClick={() => firstDiff && view(firstDiff)}
          >
            View changes
          </button>
        </div>
      </header>
      <ul>
        {visible.map((change, index) => {
          const action = editDiffAction(change);
          const location = change.resource;
          const label =
            location.kind === "resource"
              ? `${rootDisplayName(rootLabels, location.resource.rootId)} / ${location.resource.relativePath}`
              : location.label;
          return (
            <li key={index}>
              <button
                type="button"
                disabled={!open || !action.resource}
                title={
                  !open
                    ? unavailable
                    : !action.resource
                      ? action.unavailable
                      : change.diffRef
                        ? label
                        : `${label}: ${action.title}`
                }
                onClick={() => view(change)}
              >
                <span className="acp-edited-files__path">{label}</span>
                <FileChangeTotals changes={[change]} />
              </button>
            </li>
          );
        })}
      </ul>
      {changes.length > 3 ? (
        <button
          className="acp-edited-files__more"
          type="button"
          aria-expanded={expanded}
          onClick={() => setExpanded(!expanded)}
        >
          {expanded
            ? "Show fewer files"
            : `Show ${changes.length - 3} more files`}
          <ChevronDown size={12} aria-hidden="true" />
        </button>
      ) : null}
      {omittedCount > 0 ? (
        <p>{omittedCount} additional changed files not shown.</p>
      ) : null}
    </section>
  );
}
