import { useState } from "react";

import type { AcpItem } from "../../../domain/acp";
import type { AgentArtifactOpenHandler } from "../../../domain/agentArtifact";
import { ToolItem } from "./ToolItem";
import {
  toolRollupLabel,
  toolRollupState,
  toolRollupStateLabel,
  type ToolRollup,
} from "./toolActivityRollup";

export function ToolActivityRollup({
  rollup,
  linkedResults,
  interactive,
  onOpenResource,
}: {
  rollup: ToolRollup;
  linkedResults: ReadonlyMap<
    AcpItem["id"],
    Extract<AcpItem, { kind: "toolUpdateResult" }>
  >;
  interactive: boolean;
  onOpenResource?: AgentArtifactOpenHandler;
}) {
  const [expanded, setExpanded] = useState(false);
  const label = toolRollupLabel(rollup.kind, rollup.items.length);
  const state = toolRollupState(rollup.items);
  const stateLabel = toolRollupStateLabel(state);
  return (
    <details
      className="acp-activity-card acp-activity-card--tool-rollup acp-tool-rollup"
      open={expanded}
      data-activity-kind={rollup.kind}
      data-activity-state={state}
    >
      <summary
        aria-expanded={expanded}
        onClick={() => setExpanded((current) => !current)}
      >
        <span>{label}</span>
        <span className="acp-state">
          {stateLabel} · {expanded ? "Hide" : "Show"}
        </span>
      </summary>
      {expanded ? (
        <div className="acp-tool-rollup__items">
          {rollup.items.map((item) => (
            <ToolItem
              key={item.id}
              item={item}
              linkedResult={
                item.kind === "toolCall"
                  ? linkedResults.get(item.id)
                  : undefined
              }
              allowBrowse={interactive}
              onOpenResource={interactive ? onOpenResource : undefined}
            />
          ))}
        </div>
      ) : null}
    </details>
  );
}
