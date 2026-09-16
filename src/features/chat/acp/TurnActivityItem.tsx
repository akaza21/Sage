import { useState } from "react";
import { Check, Circle, LoaderCircle, XCircle } from "lucide-react";

import type { AcpItem } from "../../../domain/acp";
import {
  agentRailClockMs,
  useAgentRailViewPrefs,
} from "../../../workbench/agentRailViewPrefs";
import { ChatMarkdown } from "./ChatMarkdown";
import { terminalItemLifecycles } from "./statusSemantics";
import { isActiveTool, isWorkingTool, workingToolStatus } from "./workingTool";

type TurnActivityProjectionItem = Extract<
  AcpItem,
  { kind: "thoughtSummary" | "plan" | "usage" | "error" }
>;

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
    case "usage":
      return <UsageActivity item={item} />;
    case "error":
      return <ErrorActivity item={item} />;
  }
}
