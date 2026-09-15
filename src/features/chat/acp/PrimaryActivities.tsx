import type { ReactNode } from "react";

import type { AcpItem } from "../../../domain/acp";
import type { AgentArtifactOpenHandler } from "../../../domain/agentArtifact";
import { ToolActivityRollup } from "./ToolActivityRollupView";
import { groupPrimaryActivities } from "./toolActivityRollup";

export function PrimaryActivities({
  items,
  linkedResults,
  interactive,
  onOpenResource,
  renderItem,
}: {
  items: readonly AcpItem[];
  linkedResults: ReadonlyMap<
    AcpItem["id"],
    Extract<AcpItem, { kind: "toolUpdateResult" }>
  >;
  interactive: boolean;
  onOpenResource?: AgentArtifactOpenHandler;
  renderItem: (
    item: AcpItem,
    linkedResult?: Extract<AcpItem, { kind: "toolUpdateResult" }>,
  ) => ReactNode;
}) {
  const grouped = groupPrimaryActivities(items);
  return (
    <>
      {grouped.map((activity) =>
        activity.kind === "toolRollup" ? (
          <ToolActivityRollup
            key={activity.rollup.items[0]?.id}
            rollup={activity.rollup}
            linkedResults={linkedResults}
            interactive={interactive}
            onOpenResource={onOpenResource}
          />
        ) : (
          renderItem(
            activity.item,
            activity.item.kind === "toolCall"
              ? linkedResults.get(activity.item.id)
              : undefined,
          )
        ),
      )}
    </>
  );
}
