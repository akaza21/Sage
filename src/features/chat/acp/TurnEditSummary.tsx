import { ChevronRight, Files } from "lucide-react";

import type { AcpChatPage, TurnProjection } from "../../../domain/acp";
import { resourceKey } from "../../../domain/resourceIdentity";
import type { ResourceRef } from "../../../domain/workbench";
import { projectAgentEditSummary } from "./agentEditSummary";

interface TurnEditSummaryProps {
  page: AcpChatPage;
  turn: TurnProjection;
  rootNames: Readonly<Record<string, string>>;
  onOpenDiff?: (resource: ResourceRef) => void;
}

function rootDisplayName(
  rootNames: Readonly<Record<string, string>>,
  rootId: string,
): string {
  const projected = Object.prototype.hasOwnProperty.call(rootNames, rootId)
    ? rootNames[rootId]
    : null;
  return typeof projected === "string" && projected.length > 0
    ? projected
    : rootId;
}

export function TurnEditSummary({
  page,
  turn,
  rootNames,
  onOpenDiff,
}: TurnEditSummaryProps) {
  const summary = projectAgentEditSummary(page, turn);
  if (
    summary.changes.length === 0 &&
    !summary.sourceTruncated &&
    !summary.rejectedUnsafeOrStale
  ) {
    return null;
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
          Edited {summary.changes.length}{" "}
          {summary.changes.length === 1 ? "file" : "files"}
        </strong>
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
                  <span>{change.summary}</span>
                </li>
              );
            }
            const resource = location.resource;
            const rootName = rootDisplayName(rootNames, resource.rootId);
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
                <span>{change.summary}</span>
              </li>
            );
          })}
        </ul>
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
