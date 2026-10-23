import { useContext, useState } from "react";
import {
  BrainCircuit,
  Circle,
  Clock3,
  FilePenLine,
  FolderOpen,
  Globe2,
  Search,
  Terminal,
  Workflow,
  ChevronDown,
  X,
} from "../../../ui/leanIcons";

import type { AgentArtifactOpenHandler } from "../../../domain/agentArtifact";
import { ToolItem } from "./ToolItem";
import { ToolRunningIndicator } from "./ToolRunningIndicator";
import { WorkedActivityVisibility } from "./WorkedActivityVisibility";
import { toolStateLabels } from "./statusSemantics";
import {
  toolRollupLabel,
  toolRollupDurationLabel,
  toolRollupState,
  type ToolRollup,
} from "./toolActivityRollup";

const activityIcons: Partial<Record<ToolRollup["kind"], typeof Workflow>> = {
  edit: FilePenLine,
  delete: FilePenLine,
  move: FilePenLine,
  execute: Terminal,
  read: FolderOpen,
  search: Search,
  fetch: Globe2,
  think: BrainCircuit,
};

export function ToolActivityRollup({
  rollup,
  interactive,
  expandDetails = false,
  onOpenResource,
}: {
  rollup: ToolRollup;
  interactive: boolean;
  expandDetails?: boolean;
  onOpenResource?: AgentArtifactOpenHandler;
}) {
  const visible = useContext(WorkedActivityVisibility);
  const [openOverride, setOpenOverride] = useState<boolean | null>(null);
  const expanded = openOverride ?? (visible && expandDetails);
  const label = toolRollupLabel(
    rollup.kind,
    rollup.items,
    rollup.commandActivity,
  );
  const state = toolRollupState(rollup.items);
  const StateIcon = state === "failed" ? X : Circle;
  const ActivityIcon = activityIcons[rollup.kind] ?? Workflow;
  const duration = toolRollupDurationLabel(rollup.items);
  const live = state === "inProgress";
  const running = rollup.items.some((item) => item.toolState === "inProgress");
  return (
    <details
      className="acp-activity-card acp-activity-card--tool-rollup acp-tool-rollup"
      data-activity-kind={rollup.kind}
      data-activity-state={state}
      open={expanded}
      onToggle={(event) => {
        if (event.currentTarget.open !== expanded)
          setOpenOverride(event.currentTarget.open);
      }}
    >
      <summary>
        <span className="acp-tool-rollup__summary-copy">
          {running ? (
            <ToolRunningIndicator />
          ) : live || state === "completed" ? (
            <ActivityIcon
              aria-hidden="true"
              size={14}
              data-activity-icon={rollup.kind}
            />
          ) : (
            <StateIcon aria-hidden="true" size={14} />
          )}
          <span
            className={`acp-tool-rollup__label${live ? " acp-live-shimmer" : ""}`}
          >
            {label}
          </span>
        </span>
        <span className="acp-tool-rollup__meta">
          {duration ? (
            <span className="acp-tool-rollup__duration">
              <Clock3 aria-hidden="true" size={12} />
              {duration}
            </span>
          ) : null}
          {live ||
          (rollup.kind === "execute" && state !== "completed") ? null : (
            <span className="acp-state">{toolStateLabels[state]}</span>
          )}
          <ChevronDown
            className="acp-tool-rollup__chevron"
            aria-hidden="true"
            size={14}
          />
        </span>
      </summary>
      {expanded ? (
        <div className="acp-tool-rollup__items">
          {rollup.items.map((item) =>
            item.payload.presentation.kind === "execute" &&
            rollup.items.length > 1 ? (
              <ToolActivityRollup
                key={item.id}
                rollup={{
                  kind: "execute",
                  commandActivity: item.payload.presentation.activity,
                  items: [item],
                }}
                interactive={interactive}
                onOpenResource={onOpenResource}
              />
            ) : (
              <ToolItem
                key={item.id}
                item={item}
                compact
                allowBrowse={interactive}
                onOpenResource={interactive ? onOpenResource : undefined}
              />
            ),
          )}
        </div>
      ) : null}
    </details>
  );
}
