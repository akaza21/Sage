import { useContext, type ReactNode } from "react";
import { type AcpItem } from "../../../domain/acp";
import { type AgentArtifactOpenHandler } from "../../../domain/agentArtifact";
import { CollaborationItem } from "./CollaborationItem";
import { ToolActivityRollup } from "./ToolActivityRollupView";
import { WorkedActivityVisibility } from "./WorkedActivityVisibility";
import { withSubagentNames } from "./subagentIdentities";
import {
  groupPrimaryActivities,
  type PrimaryActivityItem,
} from "./toolActivityRollup";

export function PrimaryActivities({
  items,
  running = false,
  interactive,
  onOpenResource,
  renderItem,
}: {
  items: readonly AcpItem[];
  running?: boolean;
  interactive: boolean;
  onOpenResource?: AgentArtifactOpenHandler;
  renderItem: (item: AcpItem) => ReactNode;
}) {
  const visible = useContext(WorkedActivityVisibility);
  const grouped = groupPrimaryActivities(withSubagentNames(items));
  const last = grouped.at(-1);
  const thinking =
    running &&
    (last?.kind === "collaborationRollup" ||
      (last?.kind === "toolRollup" && last.rollup.kind === "subagent"));
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
        ) : !visible && !isSteeredMessage(activity) ? null : activity.kind ===
          "collaborationRollup" ? (
          <CollaborationItem
            key={activity.items[0].id}
            item={activity.items[0]}
            items={activity.items}
          />
        ) : (
          renderItem(activity.item)
        ),
      )}
      {thinking ? (
        <p
          className="acp-thought acp-live-shimmer"
          data-activity-kind="thought"
        >
          Thinking
        </p>
      ) : null}
    </>
  );
}

/** A steered message is the user's own words, never folded away. */
function isSteeredMessage(activity: PrimaryActivityItem) {
  return activity.kind === "item" && activity.item.kind === "userPrompt";
}
