import type { AcpChatPage, AcpItem, TurnProjection } from "../../../domain/acp";
import type {
  ProjectRootSummary,
  ResourceRef,
} from "../../../domain/workbench";

import { AcpAnnouncementRegions } from "./AcpAnnouncementRegions";
import { TurnEditSummary } from "./TurnEditSummary";
import { recordAgentEditDisposition } from "./agentEditDisposition";
import { CollaborationItem } from "./CollaborationItem";
import { HarnessActivity } from "./HarnessActivity";
import { PermissionItem } from "./PermissionItem";
import { ToolItem, type OpenResourceTarget } from "./ToolItem";
import { UnknownItem } from "./UnknownItem";
import { turnStateLabels } from "./statusSemantics";
import "./acp-chat.css";

interface AcpChatProjectionProps {
  page: AcpChatPage;
  active: boolean;
  onOpenResource?: (resource: ResourceRef, target: OpenResourceTarget) => void;
  hasEarlier?: boolean;
  loadingEarlier?: boolean;
  historyError?: string | null;
  onLoadEarlier?: () => void;
  projectRoots?: readonly Pick<ProjectRootSummary, "id" | "name" | "ordinal">[];
  retentionIncomplete?: boolean;
}

function projectRootLabels(
  roots: readonly Pick<ProjectRootSummary, "id" | "name" | "ordinal">[],
): Readonly<Record<string, string>> {
  const nameCounts = new Map<string, number>();
  for (const root of roots) {
    nameCounts.set(root.name, (nameCounts.get(root.name) ?? 0) + 1);
  }
  return Object.fromEntries(
    roots.map((root) => [
      root.id,
      (nameCounts.get(root.name) ?? 0) > 1
        ? `${root.name} (root ${root.ordinal + 1})`
        : root.name,
    ]),
  );
}

function ItemView({
  item,
  onOpenResource,
}: {
  item: AcpItem;
  onOpenResource?: AcpChatProjectionProps["onOpenResource"];
}) {
  switch (item.kind) {
    case "text":
      return (
        <article className={`acp-message acp-message--${item.payload.role}`}>
          <span className="acp-message__role">
            {item.payload.role === "assistant"
              ? "Agent"
              : item.payload.role === "user"
                ? "You"
                : "System"}
          </span>
          <p>{item.payload.text}</p>
        </article>
      );
    case "thoughtSummary":
      return (
        <details className="acp-details acp-thought">
          <summary>Thought summary</summary>
          <p>{item.payload.summary}</p>
        </details>
      );
    case "plan":
      return (
        <section className="acp-plan" aria-label="Agent plan">
          <strong>Plan</strong>
          <ol>
            {item.payload.entries.map((entry, index) => (
              <li key={`${index}:${entry.label}`}>
                <span>{entry.label}</span>
                <span>{entry.state}</span>
              </li>
            ))}
          </ol>
        </section>
      );
    case "toolCall":
    case "toolUpdateResult":
      return <ToolItem item={item} onOpenResource={onOpenResource} />;
    case "fileChangeDiff":
      return (
        <p className="acp-activity__edit-reference">
          Edit included in the Turn changed-file summary
        </p>
      );
    case "usage":
      return (
        <p className="acp-usage">
          Usage: {item.payload.totalTokens?.toLocaleString() ?? "not reported"}{" "}
          tokens
        </p>
      );
    case "permissionRequest":
      return <PermissionItem item={item} />;
    case "error":
      return (
        <article className="acp-error" role="alert" aria-live="off">
          <strong>{item.payload.code}</strong>
          <p>{item.payload.message}</p>
        </article>
      );
    case "unsupported":
      return <UnknownItem item={item} />;
    case "collaboration":
      return <CollaborationItem item={item} />;
  }
}

function Turn({
  page,
  turn,
  rootLabels,
  countIncomplete,
  onOpenResource,
}: {
  page: AcpChatPage;
  turn: TurnProjection;
  rootLabels: Readonly<Record<string, string>>;
  countIncomplete: boolean;
  onOpenResource?: AcpChatProjectionProps["onOpenResource"];
}) {
  const primaryItems = turn.items.filter(
    (item) => item.origin === "primary" && item.kind !== "fileChangeDiff",
  );
  const activityIds = new Set(turn.activities.map((activity) => activity.id));
  const itemActivities = new Map(
    turn.items.map((item) => [item.id, item.activityId] as const),
  );
  const activityRoots = turn.activities.filter(
    (activity) =>
      !activity.parentActivityId ||
      !activityIds.has(activity.parentActivityId) ||
      itemActivities.get(activity.parentItemId) !== activity.parentActivityId,
  );
  const status = turnStateLabels[turn.state];
  return (
    <section className="acp-turn" aria-label={`Turn ${turn.ordinal}`}>
      <header className="acp-turn__header">
        <span
          className="acp-state"
          role="status"
          aria-label={status}
          aria-live="off"
          aria-atomic="true"
          data-turn-status={turn.id}
          tabIndex={-1}
        >
          {status}
        </span>
      </header>
      <div className="acp-turn__items">
        {primaryItems.map((item) => (
          <ItemView key={item.id} item={item} onOpenResource={onOpenResource} />
        ))}
        {activityRoots.map((activity) => (
          <HarnessActivity
            key={activity.id}
            activity={activity}
            activities={turn.activities}
            items={turn.items}
            renderItem={(item) => (
              <ItemView item={item} onOpenResource={onOpenResource} />
            )}
          />
        ))}
        <TurnEditSummary
          page={page}
          turn={turn}
          rootLabels={rootLabels}
          countIncomplete={countIncomplete}
          onOpenDiff={
            onOpenResource
              ? (resource) => onOpenResource(resource, "diff")
              : undefined
          }
          onRecordDisposition={recordAgentEditDisposition}
        />
      </div>
    </section>
  );
}

export function AcpChatProjection({
  page,
  active,
  onOpenResource,
  hasEarlier = false,
  loadingEarlier = false,
  historyError = null,
  onLoadEarlier,
  projectRoots = [],
  retentionIncomplete = false,
}: AcpChatProjectionProps) {
  if (!active) return null;
  const earlierHistoryIncomplete = page.nextCursor !== null;
  const countIncomplete = earlierHistoryIncomplete || retentionIncomplete;
  const rootLabels = projectRootLabels(projectRoots);
  return (
    <section
      className="acp-chat-projection"
      aria-label={`Agent chat ${page.title}`}
      data-projection-revision={page.projectionRevision}
    >
      <AcpAnnouncementRegions page={page} />
      <h2 className="acp-chat-projection__title">{page.title}</h2>
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
        <p className="acp-history-error" role="alert" aria-live="off">
          Earlier history could not be loaded: {historyError}
        </p>
      ) : null}
      {earlierHistoryIncomplete ? (
        <p className="acp-history-notice">
          Earlier history has not been loaded. Changed-file summaries show
          retained records only.
        </p>
      ) : null}
      {retentionIncomplete ? (
        <p className="acp-history-notice">
          Some Chat history was discarded to stay within memory limits.
          Changed-file summaries show retained records only.
        </p>
      ) : null}
      {page.turns.length === 0 ? (
        <p className="acp-empty">No turns in this chat yet.</p>
      ) : (
        page.turns.map((turn) => (
          <Turn
            key={turn.id}
            page={page}
            turn={turn}
            rootLabels={rootLabels}
            countIncomplete={countIncomplete}
            onOpenResource={onOpenResource}
          />
        ))
      )}
    </section>
  );
}
