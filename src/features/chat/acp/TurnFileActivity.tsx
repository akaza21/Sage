import { useMemo, useState } from "react";
import { ChevronDown, FilePenLine, RotateCcw } from "../../../ui/leanIcons";
import type { AcpChatPage, TurnProjection } from "../../../domain/acp";
import type { AgentArtifactOpenHandler } from "../../../domain/agentArtifact";
import { projectAgentEditSummary } from "./agentEditSummary";
import { FileChangeTotals } from "./FileChangeTotals";
import "./edited-files-card.css";

export function TurnFileActivity({
  page,
  turn,
  onOpenResource,
}: {
  page: AcpChatPage;
  turn: TurnProjection;
  onOpenResource?: AgentArtifactOpenHandler;
}) {
  const [expanded, setExpanded] = useState(false);
  const {
    changes,
    changeKeys,
    sourceTruncated,
    rejectedUnsafeOrStale,
    omittedCount,
  } = useMemo(() => projectAgentEditSummary(page, turn), [page, turn]);
  if (!changes.length) return null;
  const incomplete = sourceTruncated || rejectedUnsafeOrStale;
  const count = changeKeys.size;
  const open =
    turn.sessionId === page.currentSessionId ? onOpenResource : undefined;
  const firstDiff = changes.find((change) => change.diffRef);
  const visible = expanded ? changes : changes.slice(0, 3);
  const unavailable = "Saved changes are unavailable for this file.";
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
            disabled
            title="This session does not support reverting edits."
          >
            Undo <RotateCcw size={12} aria-hidden="true" />
          </button>
          <button
            type="button"
            disabled={!open || !firstDiff}
            title={
              !open || !firstDiff
                ? unavailable
                : "Open the first saved file diff"
            }
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
              ? location.resource.relativePath
              : location.label;
          return (
            <li key={index}>
              <button
                type="button"
                disabled={!open || !change.diffRef}
                title={!open || !change.diffRef ? unavailable : label}
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
