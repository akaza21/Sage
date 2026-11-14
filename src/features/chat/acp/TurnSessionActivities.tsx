import type { TurnSessionActivity } from "../../../domain/acp";
import { Check, Circle, CircleX, LoaderCircle } from "../../../ui/leanIcons";

const stateLabels: Record<TurnSessionActivity["state"], string> = {
  running: "Running",
  completed: "Completed",
  failed: "Failed",
  cancelled: "Cancelled",
};
const stateIcons = { completed: Check, failed: CircleX, cancelled: Circle };

// Session activities carry no order relative to the Turn's items, so they
// follow them.
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
          >
            {Icon ? (
              <Icon size={14} aria-hidden="true" />
            ) : (
              <span className="acp-tool-running" aria-hidden="true">
                <LoaderCircle size={12} aria-hidden="true" />
              </span>
            )}
            <span className={running ? "acp-live-shimmer" : undefined}>
              {activity.label}
            </span>
            <span className="sr-only">{`, ${stateLabels[activity.state]}${activity.progress ? ": " : ""}`}</span>
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
