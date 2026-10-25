import { useContext, type ReactNode } from "react";
import { WorkedActivityVisibility } from "./WorkedActivityVisibility";

import type { AcpItem } from "../../../domain/acp";
import type { AgentArtifactOpenHandler } from "../../../domain/agentArtifact";
import { ToolActivityRollup } from "./ToolActivityRollupView";
import { groupPrimaryActivities } from "./toolActivityRollup";
import { CollaborationItem } from "./CollaborationItem";
import { withSubagentNames } from "./subagentIdentities";

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
  const visible = useContext(WorkedActivityVisibility);
  const grouped = groupPrimaryActivities(withSubagentNames(items));
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
        ) : !visible ? null : activity.kind === "collaborationRollup" ? (
          <CollaborationItem
            key={activity.items[0].id}
            item={activity.items[0]}
            items={activity.items}
          />
        ) : (
          renderItem(activity.item)
        ),
      )}
    </>
  );
}
