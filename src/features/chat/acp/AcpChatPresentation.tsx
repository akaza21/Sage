import { memo, useCallback, useMemo } from "react";

import type { AcpChatPage, AcpItem, TurnProjection } from "../../../domain/acp";
import type { AgentArtifactOpenHandler } from "../../../domain/agentArtifact";
import type { AgentDiffRef } from "../../../domain/agentDiff";
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
  onOpenResource?: AgentArtifactOpenHandler;
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

function rootLabelKey(roots: AcpChatPresentationProps["projectRoots"]): string {
  return roots
    .map((root) => `${root.id}\0${root.name}\0${root.ordinal}`)
    .join("\n");
}

const ItemView = memo(
  function ItemView({
    item,
    linkedToolResult,
    workingItems,
    interactive,
    onOpenResource,
  }: {
    item: AcpItem;
    linkedToolResult?: Extract<AcpItem, { kind: "toolUpdateResult" }>;
    workingItems?: readonly AcpItem[];
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
            <ChatMarkdown
              text={item.payload.text}
              streaming={
                item.lifecycle === "started" || item.lifecycle === "streaming"
              }
            />
          </article>
        );
      }
      case "thoughtSummary":
        return <TurnActivityItem item={item} workingItems={workingItems} />;
      case "plan":
      case "usage":
      case "error":
        return <TurnActivityItem item={item} />;
      case "toolCall":
      case "toolUpdateResult":
        return (
          <ToolItem
            item={item}
            linkedResult={
              item.kind === "toolCall" ? linkedToolResult : undefined
            }
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
  },
  (left, right) =>
    left.item.id === right.item.id &&
    left.item.revision === right.item.revision &&
    left.linkedToolResult?.id === right.linkedToolResult?.id &&
    left.linkedToolResult?.revision === right.linkedToolResult?.revision &&
    left.workingItems === right.workingItems &&
    left.interactive === right.interactive &&
    left.onOpenResource === right.onOpenResource,
);

const Turn = memo(
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
    const calls = new Set(
      turn.items.flatMap((item) => (item.kind === "toolCall" ? [item.id] : [])),
    );
    const linkedResults = new Map<
      AcpItem["id"],
      Extract<AcpItem, { kind: "toolUpdateResult" }>
    >();
    for (const item of turn.items) {
      if (
        item.kind !== "toolUpdateResult" ||
        !item.payload.callItemId ||
        !calls.has(item.payload.callItemId)
      )
        continue;
      const retained = linkedResults.get(item.payload.callItemId);
      if (
        !retained ||
        item.ordinal > retained.ordinal ||
        (item.ordinal === retained.ordinal &&
          item.updatedAt > retained.updatedAt)
      )
        linkedResults.set(item.payload.callItemId, item);
    }
    const presentedItems = turn.items.filter(
      (item) =>
        item.kind !== "toolUpdateResult" ||
        !item.payload.callItemId ||
        !calls.has(item.payload.callItemId),
    );
    const workingItems = presentedItems.map((item) =>
      item.kind === "toolCall" ? (linkedResults.get(item.id) ?? item) : item,
    );
    const primary = presentedItems.filter(
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
    const actionOrigin = useMemo(
      () => ({
        projectId: page.projectId,
        chatId: page.chatId,
        sessionId: turn.sessionId,
      }),
      [page.chatId, page.projectId, turn.sessionId],
    );
    const openForTurn = useCallback(
      (resource: ResourceRef, target: OpenResourceTarget) =>
        onOpenResource?.(resource, target, actionOrigin),
      [actionOrigin, onOpenResource],
    );
    const openDiffForTurn = useCallback(
      (resource: ResourceRef, diffRef: AgentDiffRef) =>
        onOpenResource?.(resource, "diff", actionOrigin, diffRef),
      [actionOrigin, onOpenResource],
    );
    const thought = presentedItems.find(
      (item) => item.kind === "thoughtSummary",
    );
    return (
      <section
        className="acp-turn"
        data-turn-state={turn.state}
        aria-label={`Turn ${turn.ordinal}`}
        data-turn-id={turn.id}
        data-session-id={turn.sessionId}
        data-runtime-epoch={turn.runtimeEpoch}
        data-turn-ordinal={turn.ordinal}
      >
        <header className="acp-turn__header">
          <span
            className="acp-state acp-turn__state"
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
            <ItemView
              item={thought}
              workingItems={workingItems}
              interactive={interactive}
            />
          ) : null}
          {primary.map((item) => (
            <ItemView
              key={item.id}
              item={item}
              linkedToolResult={
                item.kind === "toolCall"
                  ? linkedResults.get(item.id)
                  : undefined
              }
              interactive={interactive}
              onOpenResource={
                interactive && onOpenResource ? openForTurn : undefined
              }
            />
          ))}
          {roots.map((activity) => (
            <HarnessActivity
              key={activity.id}
              activity={activity}
              activities={turn.activities}
              items={presentedItems}
              renderItem={(item) => (
                <ItemView
                  item={item}
                  linkedToolResult={
                    item.kind === "toolCall"
                      ? linkedResults.get(item.id)
                      : undefined
                  }
                  interactive={interactive}
                  onOpenResource={
                    interactive && onOpenResource ? openForTurn : undefined
                  }
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
              interactive && onOpenResource ? openDiffForTurn : undefined
            }
            onRecordDisposition={recordAgentEditDisposition}
          />
        </div>
      </section>
    );
  },
  (left, right) =>
    // A replaced page that reuses a mutable Turn is not an immutable revision
    // update; render it conservatively rather than retaining stale controls.
    (left.turn !== right.turn || left.page === right.page) &&
    left.turn.id === right.turn.id &&
    left.turn.revision === right.turn.revision &&
    left.turn.items.length === right.turn.items.length &&
    left.turn.items.every(
      (item, index) =>
        item.id === right.turn.items[index]?.id &&
        item.revision === right.turn.items[index]?.revision,
    ) &&
    left.page.projectId === right.page.projectId &&
    left.page.chatId === right.page.chatId &&
    left.page.currentSessionId === right.page.currentSessionId &&
    left.labels === right.labels &&
    left.countIncomplete === right.countIncomplete &&
    left.onOpenResource === right.onOpenResource,
);

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
  const labelsKey = rootLabelKey(projectRoots);
  // The primitive key is the complete label projection; keeping it narrow
  // avoids row churn when a caller replaces an equivalent roots array.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const labels = useMemo(() => rootLabels(projectRoots), [labelsKey]);
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
            labels={labels}
            countIncomplete={countIncomplete}
            onOpenResource={onOpenResource}
          />
        ))
      )}
    </>
  );
}
