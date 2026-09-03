import { useEffect } from "react";

import type { AcpChatPage, AcpItem, TurnProjection } from "../../../domain/acp";
import type {
  ProjectRootSummary,
  ResourceRef,
} from "../../../domain/workbench";
import { chatTimestamp } from "../chatTimestamp";

import { AcpAnnouncementRegions } from "./AcpAnnouncementRegions";
import { dispatchBrowseFromAcpPage } from "./dispatchBrowse";
import { ChatMarkdown } from "./ChatMarkdown";
import { TurnEditSummary } from "./TurnEditSummary";
import { recordAgentEditDisposition } from "./agentEditDisposition";
import { CollaborationItem } from "./CollaborationItem";
import { HarnessActivity } from "./HarnessActivity";
import { PermissionItem } from "./PermissionItem";
import { ToolItem, type OpenResourceTarget } from "./ToolItem";
import { ImageItem } from "./ImageItem";
import { UnknownItem } from "./UnknownItem";
import { turnStateLabels } from "./statusSemantics";
import { TurnActivityItem } from "./TurnActivityItem";
import "./acp-chat.css";
import "./acp-chat-activities.css";
import "./acp-child-activity.css";
import "./acp-activity-cards.css";

interface AcpChatProjectionProps {
  page: AcpChatPage;
  active: boolean;
  focused?: boolean;
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
  interactive,
  onOpenResource,
}: {
  item: AcpItem;
  interactive: boolean;
  onOpenResource?: AcpChatProjectionProps["onOpenResource"];
}) {
  switch (item.kind) {
    case "text": {
      const timestamp = chatTimestamp(item.createdAt);
      return (
        <article
          className={`acp-message acp-message--${item.payload.role}`}
          data-item-id={item.id}
          data-message-role={item.payload.role}
        >
          <header className="acp-message__meta">
            <span className="acp-message__role">
              {item.payload.role === "assistant"
                ? "Agent"
                : item.payload.role === "user"
                  ? "You"
                  : "System"}
            </span>
            {timestamp ? (
              <time dateTime={timestamp.dateTime} title={timestamp.dateTime}>
                {timestamp.label}
              </time>
            ) : null}
          </header>
          <ChatMarkdown text={item.payload.text} />
        </article>
      );
    }
    case "thoughtSummary":
      return <TurnActivityItem item={item} />;
    case "plan":
      return <TurnActivityItem item={item} />;
    case "toolCall":
    case "toolUpdateResult":
      return (
        <ToolItem
          item={item}
          allowBrowse={interactive}
          onOpenResource={interactive ? onOpenResource : undefined}
        />
      );
    case "fileChangeDiff":
      return (
        <p className="acp-activity__edit-reference">
          Edit included in the Turn changed-file summary
        </p>
      );
    case "image":
      return <ImageItem item={item} />;
    case "usage":
      return <TurnActivityItem item={item} />;
    case "permissionRequest":
      return <PermissionItem item={item} />;
    case "error":
      return <TurnActivityItem item={item} />;
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
    (item) =>
      item.origin === "primary" &&
      item.kind !== "fileChangeDiff" &&
      item.kind !== "thoughtSummary",
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
  const interactive = turn.sessionId === page.currentSessionId;
  const thought = turn.items.find((item) => item.kind === "thoughtSummary");
  return (
    <section
      className="acp-turn"
      aria-label={`Turn ${turn.ordinal}`}
      data-turn-id={turn.id}
      data-session-id={turn.sessionId}
      data-runtime-epoch={turn.runtimeEpoch}
      data-turn-ordinal={turn.ordinal}
    >
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
        {thought ? (
          <TurnActivityItem item={thought} workingItems={turn.items} />
        ) : null}
        {primaryItems.map((item) => (
          <ItemView
            key={item.id}
            item={item}
            interactive={interactive}
            onOpenResource={onOpenResource}
          />
        ))}
        {activityRoots.map((activity) => (
          <HarnessActivity
            key={activity.id}
            activity={activity}
            activities={turn.activities}
            items={turn.items}
            renderItem={(item) => (
              <ItemView
                item={item}
                interactive={interactive}
                onOpenResource={onOpenResource}
              />
            )}
          />
        ))}
        <TurnEditSummary
          page={page}
          turn={turn}
          rootLabels={rootLabels}
          countIncomplete={countIncomplete}
          onOpenDiff={
            interactive && onOpenResource
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
  focused = true,
  onOpenResource,
  hasEarlier = false,
  loadingEarlier = false,
  historyError = null,
  onLoadEarlier,
  projectRoots = [],
  retentionIncomplete = false,
}: AcpChatProjectionProps) {
  useEffect(() => {
    if (!active || !focused) return;
    dispatchBrowseFromAcpPage(page);
  }, [active, focused, page]);
  if (!active) return null;
  const earlierHistoryIncomplete = page.nextCursor !== null;
  const countIncomplete = earlierHistoryIncomplete || retentionIncomplete;
  const rootLabels = projectRootLabels(projectRoots);
  return (
    <section
      className="acp-chat-projection"
      aria-label={`Agent chat ${page.title}`}
      data-projection-revision={page.projectionRevision}
      data-project-id={page.projectId}
      data-chat-id={page.chatId}
      data-current-session-id={page.currentSessionId ?? ""}
    >
      {focused ? <AcpAnnouncementRegions page={page} /> : null}
      <h2 className="sr-only">{page.title}</h2>
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
