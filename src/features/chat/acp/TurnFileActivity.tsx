import { useMemo } from "react";
import { FilePenLine } from "../../../ui/leanIcons";
import type { AcpChatPage, TurnProjection } from "../../../domain/acp";
import { projectAgentEditSummary } from "./agentEditSummary";
import { FileChangeTotals } from "./FileChangeTotals";

export function TurnFileActivity({
  page,
  turn,
}: {
  page: AcpChatPage;
  turn: TurnProjection;
}) {
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
  return (
    <div className="acp-activity-row acp-edited-files">
      <FilePenLine size={14} aria-hidden="true" />
      <span>
        Edited {incomplete ? "at least " : ""}
        {count} {count === 1 ? "file" : "files"}
      </span>
      <FileChangeTotals
        changes={changes}
        incomplete={incomplete || omittedCount > 0}
      />
    </div>
  );
}
