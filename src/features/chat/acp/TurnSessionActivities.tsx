import type { TurnSessionActivity } from "../../../domain/acp";
import { Check, Circle, CircleX } from "../../../ui/leanIcons";
import { ToolRunningIndicator } from "./ToolRunningIndicator";

const stateLabels: Record<TurnSessionActivity["state"], string> = {
  running: "Running",
  completed: "Completed",
  failed: "Failed",
  cancelled: "Cancelled",
};
const stateIcons = { completed: Check, failed: CircleX, cancelled: Circle };

/**
 * The Turn's session activity rows (hook runs), one compact line each with
 * the harness's status or failure line. Rust bounds the list to the session
 * surface's activity cap; the rows carry no order relative to the Turn's
 * items, so they follow them.
 */
export function TurnSessionActivities({
  activities,
}: {
  activities: readonly TurnSessionActivity[];
}) {
  if (activities.length === 0) return null;
  return (
    <ul className="acp-session-activities" aria-label="Hooks">
      {activities.map((activity, index) => {
        const running = activity.state === "running";
        const Icon =
          activity.state === "running" ? null : stateIcons[activity.state];
        return (
          <li
            key={`${index}:${activity.label}`}
            className="acp-activity-row acp-session-activity"
            data-activity-kind={activity.kind}
            data-activity-state={activity.state}
            aria-label={[
              activity.label,
              stateLabels[activity.state],
              activity.progress,
            ]
              .filter(Boolean)
              .join(", ")}
          >
            {Icon ? (
              <Icon size={14} aria-hidden="true" />
            ) : (
              <ToolRunningIndicator />
            )}
            <span className={running ? "acp-live-shimmer" : undefined}>
              {activity.label}
            </span>
            {activity.progress ? (
              <span className="acp-session-activity__detail">
                {activity.progress}
              </span>
            ) : null}
          </li>
        );
      })}
    </ul>
  );
}
