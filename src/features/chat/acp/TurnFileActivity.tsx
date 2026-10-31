import { useMemo, useState } from "react";
import type { AcpChatPage, TurnProjection } from "../../../domain/acp";
import type { AgentArtifactOpenHandler } from "../../../domain/agentArtifact";
import { ChevronDown, FilePenLine } from "../../../ui/leanIcons";
import { projectTurnEditSummary } from "./chatEditSummary";
import "./edited-files-card.css";
import { FileChangeTotals } from "./FileChangeTotals";
import { rootDisplayName } from "./rootDisplayName";
import type { TurnWorkspaceChange } from "./turnWorkspaceChanges";

const INITIAL_FILE_ROWS = 3;

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
  // With no known file, an incomplete ledger is reported on its own rather
  // than as an "Edited at least 0 files" card with dead actions.
  if (!changes.length)
    return (
      <p role="status">
        Workspace change tracking is incomplete for this turn.
      </p>
    );
  const incomplete = sourceTruncated || rejectedUnsafeOrStale;
  const count = changeKeys.size;
  const open =
    turn.sessionId === page.currentSessionId ? onOpenResource : undefined;
  const firstDiff = changes.find((change) => change.diffRef);
  const visible = expanded ? changes : changes.slice(0, INITIAL_FILE_ROWS);
  const single = changes[0].resource;
  const basename =
    count === 1 &&
    !incomplete &&
    (single.kind === "resource" ? single.resource.relativePath : single.label)
      .split(/[\\/]/)
      .pop();
  const unavailable = "No saved diff.";
  function view(change: (typeof changes)[number]) {
    if (!open || !change.diffRef || change.resource.kind !== "resource") return;
    open(
      change.resource.resource,
      "diff",
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
      {workspaceIncomplete ? (
        <p role="status">
          Workspace change tracking is incomplete for this turn.
        </p>
      ) : null}
      <header className="acp-edited-files__header">
        <span className="acp-edited-files__icon">
          <FilePenLine size={16} aria-hidden="true" />
        </span>
        <div className="acp-edited-files__heading">
          <strong title={basename || undefined}>
            {basename
              ? `Edited ${basename}`
              : `Edited ${incomplete ? "at least " : ""}${count} file${count === 1 ? "" : "s"}`}
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
            title={open && firstDiff ? "Open saved diff" : unavailable}
            onClick={() => firstDiff && view(firstDiff)}
          >
            View changes
          </button>
        </div>
      </header>
      <ul>
        {visible.map((change, index) => {
          const location = change.resource;
          const label =
            location.kind === "resource"
              ? `${rootDisplayName(rootLabels, location.resource.rootId)} / ${location.resource.relativePath}`
              : location.label;
          return (
            <li key={index}>
              <button
                type="button"
                disabled={!open || !change.diffRef}
                title={
                  !open || !change.diffRef ? `${label}: ${unavailable}` : label
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
      {changes.length > INITIAL_FILE_ROWS ? (
        <button
          className="acp-edited-files__more"
          type="button"
          aria-expanded={expanded}
          onClick={() => setExpanded(!expanded)}
        >
          {expanded
            ? "Show fewer files"
            : `Show ${changes.length - INITIAL_FILE_ROWS} more files`}
          <ChevronDown size={12} aria-hidden="true" />
        </button>
      ) : null}
      {omittedCount > 0 ? (
        <p>{omittedCount} additional changed files not shown.</p>
      ) : null}
    </section>
  );
}
