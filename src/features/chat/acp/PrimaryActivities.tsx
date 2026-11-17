import { type ReactNode } from "react";
import { WorkRow } from "../WorkRow";
import { type AcpItem } from "../../../domain/acp";
import { type AgentArtifactOpenHandler } from "../../../domain/agentArtifact";
import { CollaborationItem } from "./CollaborationItem";
import { ToolActivityRollup } from "./ToolActivityRollupView";
import { withSubagentNames } from "./subagentIdentities";
import { groupPrimaryActivities } from "./toolActivityRollup";

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
  const grouped = groupPrimaryActivities(withSubagentNames(items));
  const last = grouped.at(-1);
  const isMessage = (activity: (typeof grouped)[number] | undefined) =>
    activity?.kind === "item" &&
    (activity.item.kind === "text" || activity.item.kind === "userPrompt");
  const thinking =
    running &&
    (last?.kind === "collaborationRollup" ||
      (last?.kind === "toolRollup" && last.rollup.kind === "subagent"));
  return (
    <>
      {grouped.map((activity, index) => (
        <WorkRow
          key={
            activity.kind === "toolRollup"
              ? activity.rollup.items[0]?.id
              : activity.kind === "collaborationRollup"
                ? activity.items[0].id
                : activity.item.id
          }
          afterMessage={
            activity.kind === "item" &&
            activity.item.kind === "thoughtSummary" &&
            isMessage(grouped[index - 1])
          }
        >
          {activity.kind === "toolRollup" ? (
            <ToolActivityRollup
              rollup={activity.rollup}
              interactive={interactive}
              onOpenResource={onOpenResource}
            />
          ) : activity.kind === "collaborationRollup" ? (
            <CollaborationItem
              item={activity.items[0]}
              items={activity.items}
            />
          ) : (
            renderItem(activity.item)
          )}
        </WorkRow>
      ))}
      {thinking ? (
        <WorkRow afterMessage={isMessage(last)}>
          <p
            className="acp-thought acp-live-shimmer"
            data-activity-kind="thought"
          >
            Thinking
          </p>
        </WorkRow>
      ) : null}
    </>
  );
}
