import { memo, useState } from "react";
import { Check, Circle, LoaderCircle, XCircle } from "lucide-react";

import type { AcpItem } from "../../../domain/acp";
import type {
  AgentArtifactOpenHandler,
  AgentArtifactOwner,
} from "../../../domain/agentArtifact";
import { chatTimestamp } from "../chatTimestamp";
import {
  agentRailClockMs,
  useAgentRailViewPrefs,
} from "../../../workbench/agentRailViewPrefs";
import { ChatMarkdown } from "./ChatMarkdown";
import { CollaborationItem } from "./CollaborationItem";
import { ImageItem } from "./ImageItem";
import { PermissionItem } from "./PermissionItem";
import { ToolItem } from "./ToolItem";
import { UnknownItem } from "./UnknownItem";
import { terminalItemLifecycles } from "./statusSemantics";
import { isActiveTool, isWorkingTool, workingToolStatus } from "./workingTool";

type TurnActivityProjectionItem = Extract<
  AcpItem,
  { kind: "thoughtSummary" | "plan" | "error" }
>;

export const ItemView = memo(
  function ItemView({
    item,
    workingItems,
    interactive,
    fileReferenceOwner,
    onOpenResource,
  }: {
    item: AcpItem;
    workingItems?: readonly AcpItem[];
    interactive: boolean;
    fileReferenceOwner?: AgentArtifactOwner;
    onOpenResource?: AgentArtifactOpenHandler;
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
              {item.payload.role === "assistant" ? (
                <span className="acp-message__avatar" aria-hidden="true">
                  S
                </span>
              ) : null}
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
              fileReferenceOwner={
                item.payload.role === "assistant"
                  ? fileReferenceOwner
                  : undefined
              }
            />
          </article>
        );
      }
      case "thoughtSummary":
      case "plan":
        return <TurnActivityItem item={item} workingItems={workingItems} />;
      case "error":
        return <TurnActivityItem item={item} />;
      case "toolCall":
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
  },
  (left, right) =>
    left.item.id === right.item.id &&
    left.item.revision === right.item.revision &&
    (left.item.kind !== "thoughtSummary" && left.item.kind !== "plan"
      ? true
      : left.workingItems === right.workingItems) &&
    left.interactive === right.interactive &&
    left.fileReferenceOwner === right.fileReferenceOwner &&
    left.onOpenResource === right.onOpenResource,
);

const MAX_THOUGHT_WORKING_ROWS = 64;
const MAX_PLAN_WORKING_ROWS = 3;
const planStateLabels = {
  pending: "Pending",
  inProgress: "In progress",
  completed: "Completed",
  failed: "Failed",
} as const;

function PlanStateIcon({ state }: { state: keyof typeof planStateLabels }) {
  if (state === "completed") return <Check size={14} aria-hidden="true" />;
  if (state === "inProgress")
    return (
      <LoaderCircle
        className="acp-plan__working"
        size={14}
        aria-hidden="true"
      />
    );
  if (state === "failed") return <XCircle size={14} aria-hidden="true" />;
  return <Circle size={14} aria-hidden="true" />;
}

function thoughtDurationLabel(createdAt: number, endAt: number): string | null {
  if (
    !Number.isFinite(createdAt) ||
    !Number.isFinite(endAt) ||
    endAt < createdAt
  )
    return null;
  const seconds = Math.floor((endAt - createdAt) / 1_000);
  return seconds > 0 ? `Thought ${seconds}s` : "Thought briefly";
}

function ThoughtActivity({
  item,
  workingItems,
  nowMs,
}: {
  item: Extract<AcpItem, { kind: "thoughtSummary" }>;
  workingItems: readonly AcpItem[];
  nowMs?: number;
}) {
  useAgentRailViewPrefs();
  const liveNowMs = nowMs ?? agentRailClockMs();
  const active = workingItems.filter(isActiveTool);
  const working = workingItems
    .filter(isWorkingTool)
    .slice(0, MAX_THOUGHT_WORKING_ROWS);
  const terminal = terminalItemLifecycles.has(item.lifecycle);
  const endAt = terminal ? (item.terminalAt ?? item.updatedAt) : liveNowMs;
  const label = thoughtDurationLabel(item.createdAt, endAt);
  return (
    <details
      className="acp-activity-card acp-activity-card--thought acp-details acp-thought"
      data-activity-kind="thought"
      data-activity-state={active.length > 0 ? "inProgress" : "completed"}
    >
      <summary>
        <span>{label ?? "Thought"}</span>
        <span className="acp-state">
          {active.length > 0 ? "In progress" : "Details"}
        </span>
      </summary>
      <ChatMarkdown
        text={item.payload.summary}
        streaming={!terminalItemLifecycles.has(item.lifecycle)}
      />
      {working.length > 0 ? (
        <ul className="acp-thought__working" aria-label="Working tools">
          {working.map((workingItem) => (
            <li key={workingItem.id}>
              <span className="acp-thought__pulse" aria-hidden="true" />
              <span className="acp-thought__working-copy">
                <span>{workingItem.payload.presentation.label}</span>
                <span>{workingToolStatus(workingItem)}</span>
              </span>
            </li>
          ))}
        </ul>
      ) : null}
    </details>
  );
}

function PlanActivity({
  item,
  workingItems,
}: {
  item: Extract<AcpItem, { kind: "plan" }>;
  workingItems: readonly AcpItem[];
}) {
  const [expanded, setExpanded] = useState(true);
  const complete = item.payload.entries.filter(
    (entry) => entry.state === "completed",
  ).length;
  const activityState = item.payload.entries.some(
    (entry) => entry.state === "failed",
  )
    ? "failed"
    : complete === item.payload.entries.length
      ? "completed"
      : "inProgress";
  const inProgressIndex = item.payload.entries.findIndex(
    (entry) => entry.state === "inProgress",
  );
  const working = workingItems.filter(isWorkingTool);
  const visibleWorking = working.slice(0, MAX_PLAN_WORKING_ROWS);
  const omittedWorkingCount = working.length - visibleWorking.length;
  return (
    <section
      className="acp-activity-card acp-activity-card--plan acp-plan"
      aria-label="Agent plan"
      data-activity-kind="plan"
      data-activity-state={activityState}
    >
      <button
        className="acp-plan__summary"
        type="button"
        aria-expanded={expanded}
        onClick={() => setExpanded((current) => !current)}
      >
        <strong>Plan</strong>
        <span className="acp-state" role="status" aria-live="off">
          {expanded
            ? `Hide · ${complete} of ${item.payload.entries.length} complete`
            : `${complete} of ${item.payload.entries.length} complete`}
        </span>
      </button>
      {expanded ? (
        <ol>
          {item.payload.entries.map((entry, index) => (
            <li key={`${index}:${entry.label}`} data-plan-state={entry.state}>
              <PlanStateIcon state={entry.state} />
              <span>{entry.label}</span>
              <span className="acp-plan__state">
                {planStateLabels[entry.state]}
              </span>
              {index === inProgressIndex && visibleWorking.length > 0 ? (
                <ul
                  className="acp-plan__activity"
                  aria-label="Current activity"
                >
                  {visibleWorking.map((workingItem) => (
                    <li key={workingItem.id}>
                      <span className="acp-plan__pulse" aria-hidden="true" />
                      <span>
                        <span>{workingItem.payload.presentation.label}</span>
                        <span>{workingToolStatus(workingItem)}</span>
                      </span>
                    </li>
                  ))}
                  {omittedWorkingCount > 0 ? (
                    <li className="acp-plan__working-more">
                      {omittedWorkingCount} more activity item
                      {omittedWorkingCount === 1 ? "" : "s"} running
                    </li>
                  ) : null}
                </ul>
              ) : null}
            </li>
          ))}
        </ol>
      ) : null}
    </section>
  );
}

function ErrorActivity({
  item,
}: {
  item: Extract<AcpItem, { kind: "error" }>;
}) {
  return (
    <article
      className="acp-activity-card acp-activity-card--error acp-error"
      role="alert"
      aria-live="off"
      data-activity-kind="error"
      data-activity-state="failed"
    >
      <header>
        <strong>{item.payload.code}</strong>
        <span className="acp-state">
          {item.payload.recoverable ? "Retry available" : "Needs attention"}
        </span>
      </header>
      <p>{item.payload.message}</p>
    </article>
  );
}

export function TurnActivityItem({
  item,
  workingItems = [],
  nowMs,
}: {
  item: TurnActivityProjectionItem;
  workingItems?: readonly AcpItem[];
  nowMs?: number;
}) {
  switch (item.kind) {
    case "thoughtSummary":
      return (
        <ThoughtActivity
          item={item}
          workingItems={workingItems}
          nowMs={nowMs}
        />
      );
    case "plan":
      return <PlanActivity item={item} workingItems={workingItems} />;
    case "error":
      return <ErrorActivity item={item} />;
  }
}
