import type { ReactNode } from "react";

import type { AcpItem } from "../../../domain/acp";
import type { AgentArtifactOpenHandler } from "../../../domain/agentArtifact";
import { ToolActivityRollup } from "./ToolActivityRollupView";
import { groupPrimaryActivities } from "./toolActivityRollup";

export function PrimaryActivities({
  items,
  interactive,
  onOpenResource,
  renderItem,
}: {
  items: readonly AcpItem[];
  interactive: boolean;
  onOpenResource?: AgentArtifactOpenHandler;
  renderItem: (item: AcpItem) => ReactNode;
}) {
  const grouped = groupPrimaryActivities(items);
  return (
    <>
      {grouped.map((activity) =>
        activity.kind === "toolRollup" ? (
          <ToolActivityRollup
            key={activity.rollup.items[0]?.id}
            rollup={activity.rollup}
            interactive={interactive}
            onOpenResource={onOpenResource}
          />
        ) : (
          renderItem(activity.item)
        ),
      )}
    </>
  );
}
