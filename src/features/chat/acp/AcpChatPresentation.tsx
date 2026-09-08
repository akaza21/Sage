import type { AcpChatPage, AcpItem, TurnProjection } from "../../../domain/acp";
import type {
  ProjectRootSummary,
  ResourceRef,
} from "../../../domain/workbench";
import { chatTimestamp } from "../chatTimestamp";
import { ChatMarkdown } from "./ChatMarkdown";
import { TurnEditSummary } from "./TurnEditSummary";
import { recordAgentEditDisposition } from "./agentEditDisposition";
import { CollaborationItem } from "./CollaborationItem";
import { HarnessActivity } from "./HarnessActivity";
import { ImageItem } from "./ImageItem";
import { PermissionItem } from "./PermissionItem";
import { ToolItem, type OpenResourceTarget } from "./ToolItem";
import { TurnActivityItem } from "./TurnActivityItem";
import { UnknownItem } from "./UnknownItem";
import { turnStateLabels } from "./statusSemantics";

export type AcpChatPresentationProps = {
  page: AcpChatPage;
  onOpenResource?: (resource: ResourceRef, target: OpenResourceTarget) => void;
  hasEarlier: boolean;
  loadingEarlier: boolean;
  historyError: string | null;
  onLoadEarlier?: () => void;
  projectRoots: readonly Pick<ProjectRootSummary, "id" | "name" | "ordinal">[];
  retentionIncomplete: boolean;
};

function rootLabels(
  roots: AcpChatPresentationProps["projectRoots"],
): Readonly<Record<string, string>> {
  const counts = new Map<string, number>();
  for (const root of roots)
    counts.set(root.name, (counts.get(root.name) ?? 0) + 1);
  return Object.fromEntries(
    roots.map((root) => [
      root.id,
      (counts.get(root.name) ?? 0) > 1
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
  onOpenResource?: AcpChatPresentationProps["onOpenResource"];
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
    case "plan":
    case "usage":
    case "error":
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
    case "permissionRequest":
      return <PermissionItem item={item} />;
    case "unsupported":
      return <UnknownItem item={item} />;
    case "collaboration":
      return <CollaborationItem item={item} />;
  }
}

function Turn({
  page,
  turn,
  labels,
  countIncomplete,
  onOpenResource,
}: {
  page: AcpChatPage;
  turn: TurnProjection;
  labels: Readonly<Record<string, string>>;
  countIncomplete: boolean;
  onOpenResource?: AcpChatPresentationProps["onOpenResource"];
}) {
  const primary = turn.items.filter(
    (item) =>
      item.origin === "primary" &&
      item.kind !== "fileChangeDiff" &&
      item.kind !== "thoughtSummary",
  );
  const activityIds = new Set(turn.activities.map((activity) => activity.id));
  const itemActivities = new Map(
    turn.items.map((item) => [item.id, item.activityId] as const),
  );
  const roots = turn.activities.filter(
    (activity) =>
      !activity.parentActivityId ||
      !activityIds.has(activity.parentActivityId) ||
      itemActivities.get(activity.parentItemId) !== activity.parentActivityId,
  );
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
          aria-label={turnStateLabels[turn.state]}
          aria-live="off"
          aria-atomic="true"
          data-turn-status={turn.id}
          tabIndex={-1}
        >
          {turnStateLabels[turn.state]}
        </span>
      </header>
      <div className="acp-turn__items">
        {thought ? (
          <TurnActivityItem item={thought} workingItems={turn.items} />
        ) : null}
        {primary.map((item) => (
          <ItemView
            key={item.id}
            item={item}
            interactive={interactive}
            onOpenResource={onOpenResource}
          />
        ))}
        {roots.map((activity) => (
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
          rootLabels={labels}
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

export function AcpChatPresentation({
  page,
  onOpenResource,
  hasEarlier,
  loadingEarlier,
  historyError,
  onLoadEarlier,
  projectRoots,
  retentionIncomplete,
}: AcpChatPresentationProps) {
  const incomplete = page.nextCursor !== null;
  const countIncomplete = incomplete || retentionIncomplete;
  return (
    <>
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
      {incomplete ? (
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
            labels={rootLabels(projectRoots)}
            countIncomplete={countIncomplete}
            onOpenResource={onOpenResource}
          />
        ))
      )}
    </>
  );
}
