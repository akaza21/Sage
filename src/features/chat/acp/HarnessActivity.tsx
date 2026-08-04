import type { ReactNode } from "react";

import type { AcpItem, HarnessActivityProjection } from "../../../domain/acp";

import { CollaborationItem } from "./CollaborationItem";
import { UnknownItem } from "./UnknownItem";

const lifecycleLabels = {
  observed: "Observed",
  active: "Active",
  completed: "Completed",
  failed: "Failed",
  cancelled: "Cancelled",
  unsupported: "Unsupported",
} as const;

export function HarnessActivity({
  activity,
  items,
  renderItem,
}: {
  activity: HarnessActivityProjection;
  items: readonly AcpItem[];
  renderItem?: (item: AcpItem) => ReactNode;
}) {
  const ownedItems = items.filter((item) => item.activityId === activity.id);
  return (
    <details className="acp-activity">
      <summary>
        <span>{activity.provenance.safeLabel ?? "Harness activity"}</span>
        <span className="acp-state" role="status">
          {lifecycleLabels[activity.lifecycle]}
        </span>
      </summary>
      <p className="acp-activity__provenance">
        Harness-owned {activity.role} activity · depth {activity.depth}
      </p>
      {ownedItems.map((item) => {
        if (renderItem) return <div key={item.id}>{renderItem(item)}</div>;
        if (item.kind === "collaboration") {
          return <CollaborationItem key={item.id} item={item} />;
        }
        if (item.kind === "unsupported") {
          return <UnknownItem key={item.id} item={item} />;
        }
        return null;
      })}
    </details>
  );
}
