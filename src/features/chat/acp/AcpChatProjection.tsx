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
import {
  agentRailClockMs,
  useAgentRailViewPrefs,
} from "../../../workbench/agentRailViewPrefs";
import "./acp-chat.css";
import "./acp-chat-activities.css";

const MAX_THOUGHT_WORKING_ROWS = 64;

function thoughtLabel(createdAt: number, nowMs: number) {
  if (
    !Number.isFinite(createdAt) ||
    !Number.isFinite(nowMs) ||
    nowMs <= createdAt
  )
    return "Thought briefly";
  const seconds = Math.floor((nowMs - createdAt) / 1_000);
  return seconds > 0 ? `Thought ${seconds}s` : "Thought briefly";
}

function isWorkingTool(
  item: AcpItem,
): item is Extract<AcpItem, { kind: "toolCall" | "toolUpdateResult" }> {
  return (
    (item.kind === "toolCall" || item.kind === "toolUpdateResult") &&
    item.toolState === "inProgress"
  );
}

function workingToolStatus(
  item: Extract<AcpItem, { kind: "toolCall" | "toolUpdateResult" }>,
) {
  const presentation = item.payload.presentation;
  if (presentation.kind === "search") {
    return `Searching… ${presentation.querySummary}`;
  }
  const detail =
    item.kind === "toolCall"
      ? item.payload.inputSummary
      : item.payload.outputSummary;
  return detail || "Working…";
}

function ThoughtBlock({ turn }: { turn: TurnProjection }) {
  useAgentRailViewPrefs();
  const thought = turn.items.find((item) => item.kind === "thoughtSummary");
  if (!thought) return null;
  const working = turn.items
    .filter(isWorkingTool)
    .slice(0, MAX_THOUGHT_WORKING_ROWS);
  return (
    <details className="acp-details acp-thought">
      <summary>{thoughtLabel(thought.createdAt, agentRailClockMs())}</summary>
      <ChatMarkdown text={thought.payload.summary} />
      {working.length > 0 ? (
        <ul className="acp-thought__working" aria-label="Working tools">
          {working.map((item) => (
            <li key={item.id}>
              <span className="acp-thought__pulse" aria-hidden="true" />
              <span className="acp-thought__working-copy">
                <span>{item.payload.presentation.label}</span>
                <span>{workingToolStatus(item)}</span>
              </span>
            </li>
          ))}
        </ul>
      ) : null}
    </details>
  );
}

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
  onOpenResource,
}: {
  item: AcpItem;
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
      return (
        <details className="acp-details acp-thought">
          <summary>Thought summary</summary>
          <ChatMarkdown text={item.payload.summary} />
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
    case "image":
      return <ImageItem item={item} />;
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
        <ThoughtBlock turn={turn} />
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
