import { useState } from "react";
import type { AcpItem } from "../../../domain/acp";
import { Check, ChevronRight, XCircle } from "../../../ui/leanIcons";
import { formatDuration } from "../transcriptSummary";
import { isWorkingTool, workingToolStatus } from "./workingTool";

type PlanItem = Extract<AcpItem, { kind: "plan" }>;
type PlanState = PlanItem["payload"]["entries"][number]["state"];

const MAX_PLAN_WORKING_ROWS = 3;
// The protocol reports no skipped state, so none is drawn.
const planStateLabels: Record<PlanState, string> = {
  pending: "Pending",
  inProgress: "In progress",
  completed: "Completed",
  failed: "Failed",
};

function PlanMark({ state }: { state: PlanState }) {
  if (state === "completed")
    return <Check size={14} aria-hidden="true" className="acp-plan__mark" />;
  if (state === "failed")
    return <XCircle size={14} aria-hidden="true" className="acp-plan__mark" />;
  return <span className="acp-plan__dot" aria-hidden="true" />;
}

const completedSteps = (item: PlanItem) =>
  item.payload.entries.filter((entry) => entry.state === "completed").length;

/** The checklist shown while the Turn runs. */
export function PlanActivity({
  item,
  workingItems,
}: {
  item: PlanItem;
  workingItems: readonly AcpItem[];
}) {
  const [expanded, setExpanded] = useState(true);
  const entries = item.payload.entries;
  const complete = completedSteps(item);
  const activityState = entries.some((entry) => entry.state === "failed")
    ? "failed"
    : complete === entries.length
      ? "completed"
      : "inProgress";
  const inProgressIndex = entries.findIndex(
    (entry) => entry.state === "inProgress",
  );
  const working = workingItems.filter(isWorkingTool);
  const visibleWorking = working.slice(0, MAX_PLAN_WORKING_ROWS);
  const omittedWorkingCount = working.length - visibleWorking.length;
  return (
    <section
      className="acp-activity acp-plan"
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
        <span>Plan</span>
        <span className="acp-state" role="status" aria-live="off">
          {`${complete} of ${entries.length} complete`}
        </span>
      </button>
      {expanded ? (
        <ol>
          {entries.map((entry, index) => (
            <li key={`${index}:${entry.label}`} data-plan-state={entry.state}>
              <PlanMark state={entry.state} />
              <span>{entry.label}</span>
              <span className="sr-only">{planStateLabels[entry.state]}</span>
              {index === inProgressIndex && visibleWorking.length > 0 ? (
                <ul
                  className="acp-plan__activity"
                  aria-label="Current activity"
                >
                  {visibleWorking.map((workingItem) => (
                    <li key={workingItem.id}>
                      <span>{workingItem.payload.presentation.label}</span>
                      <span>{workingToolStatus(workingItem)}</span>
                    </li>
                  ))}
                  {omittedWorkingCount > 0 ? (
                    <li>
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

/** A settled Turn folds its latest plan into this chip beside Worked for. */
export function PlanChip({ item }: { item: PlanItem }) {
  const done = completedSteps(item);
  const total = item.payload.entries.length;
  return (
    <span className="acp-plan-chip" data-activity-kind="plan">
      <Check size={14} aria-hidden="true" />
      {`${done}/${total} steps`}
    </span>
  );
}

/** Latest plan of a settled Turn; it leaves the activity list for the chip. */
export function settledPlan(
  items: readonly AcpItem[],
  terminal: boolean,
): PlanItem | undefined {
  if (!terminal) return undefined;
  for (let index = items.length - 1; index >= 0; index -= 1) {
    const item = items[index]!;
    if (item.kind === "plan" && item.payload.entries.length > 0) return item;
  }
  return undefined;
}

/** "Thought for Ns", opening to the harness's reasoning summary. */
export function ThoughtRow({
  item,
}: {
  item: Extract<AcpItem, { kind: "thoughtSummary" }>;
}) {
  const [open, setOpen] = useState(false);
  const label =
    formatDuration("Thought", item.createdAt, item.terminalAt) ?? "Thought";
  return (
    <details
      className="acp-activity acp-tool-rollup acp-thought-row"
      data-activity-kind="thought"
      open={open}
      onToggle={(event) => setOpen(event.currentTarget.open)}
    >
      <summary>
        <span className="acp-tool-rollup__summary-copy">
          <span className="acp-tool-rollup__label">{label}</span>
        </span>
        <ChevronRight
          className="acp-tool-rollup__chevron"
          aria-hidden="true"
          size={14}
        />
      </summary>
      {open ? (
        <p className="acp-thought-summary">{item.payload.summary}</p>
      ) : null}
    </details>
  );
}
