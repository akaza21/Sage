import type { AcpItem } from "../../../domain/acp";
import {
  agentRailClockMs,
  useAgentRailViewPrefs,
} from "../../../workbench/agentRailViewPrefs";
import { ChatMarkdown } from "./ChatMarkdown";

type TurnActivityProjectionItem = Extract<
  AcpItem,
  { kind: "thoughtSummary" | "plan" | "usage" | "error" }
>;
type WorkingToolItem = Extract<
  AcpItem,
  { kind: "toolCall" | "toolUpdateResult" }
>;

const MAX_THOUGHT_WORKING_ROWS = 64;
const planStateLabels = {
  pending: "Pending",
  inProgress: "In progress",
  completed: "Completed",
  failed: "Failed",
} as const;

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

function isWorkingTool(item: AcpItem): item is WorkingToolItem {
  return (
    (item.kind === "toolCall" || item.kind === "toolUpdateResult") &&
    item.toolState === "inProgress"
  );
}

function workingToolStatus(item: WorkingToolItem) {
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

function ThoughtActivity({
  item,
  workingItems,
}: {
  item: Extract<AcpItem, { kind: "thoughtSummary" }>;
  workingItems: readonly AcpItem[];
}) {
  useAgentRailViewPrefs();
  const working = workingItems
    .filter(isWorkingTool)
    .slice(0, MAX_THOUGHT_WORKING_ROWS);
  return (
    <details
      className="acp-activity-card acp-activity-card--thought acp-details acp-thought"
      data-activity-kind="thought"
      data-activity-state={working.length > 0 ? "inProgress" : "completed"}
    >
      <summary>
        <span>{thoughtLabel(item.createdAt, agentRailClockMs())}</span>
        <span className="acp-state">
          {working.length > 0 ? "In progress" : "Details"}
        </span>
      </summary>
      <ChatMarkdown text={item.payload.summary} />
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

function PlanActivity({ item }: { item: Extract<AcpItem, { kind: "plan" }> }) {
  const complete = item.payload.entries.filter(
    (entry) => entry.state === "completed",
  ).length;
  return (
    <section
      className="acp-activity-card acp-activity-card--plan acp-plan"
      aria-label="Agent plan"
      data-activity-kind="plan"
      data-activity-state={
        item.payload.entries.some((entry) => entry.state === "failed")
          ? "failed"
          : complete === item.payload.entries.length
            ? "completed"
            : "inProgress"
      }
    >
      <header>
        <strong>Plan</strong>
        <span className="acp-state" role="status" aria-live="off">
          {complete} of {item.payload.entries.length} complete
        </span>
      </header>
      <ol>
        {item.payload.entries.map((entry, index) => (
          <li key={`${index}:${entry.label}`}>
            <span>{entry.label}</span>
            <span className="acp-state">{planStateLabels[entry.state]}</span>
          </li>
        ))}
      </ol>
    </section>
  );
}

function UsageActivity({
  item,
}: {
  item: Extract<AcpItem, { kind: "usage" }>;
}) {
  const total = item.payload.totalTokens;
  return (
    <article
      className="acp-activity-card acp-activity-card--usage acp-usage"
      aria-label="Usage"
      data-activity-kind="usage"
      data-activity-state={total === null ? "unknown" : "reported"}
    >
      <strong>Usage</strong>
      <span className="acp-state">
        {total === null
          ? "Tokens not reported"
          : `${total.toLocaleString()} tokens`}
      </span>
    </article>
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
}: {
  item: TurnActivityProjectionItem;
  workingItems?: readonly AcpItem[];
}) {
  switch (item.kind) {
    case "thoughtSummary":
      return <ThoughtActivity item={item} workingItems={workingItems} />;
    case "plan":
      return <PlanActivity item={item} />;
    case "usage":
      return <UsageActivity item={item} />;
    case "error":
      return <ErrorActivity item={item} />;
  }
}
