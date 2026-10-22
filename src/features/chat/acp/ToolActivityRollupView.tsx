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
import { compactToolLabels } from "./toolCommandContent";
import {
  toolRollupLabel,
  toolRollupDurationLabel,
  toolRollupState,
  toolRollupStateLabel,
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

function ActivityKindIcon({ kind }: { kind: ToolRollup["kind"] }) {
  const Icon = activityIcons[kind] ?? Workflow;
  return <Icon aria-hidden="true" size={14} data-activity-icon={kind} />;
}

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
  const expanded =
    openOverride ??
    (visible &&
      expandDetails &&
      rollup.items.every(
        (item) => compactToolLabels[item.payload.presentation.kind],
      ));
  const label = toolRollupLabel(
    rollup.kind,
    rollup.items,
    rollup.commandActivity,
  );
  const state = toolRollupState(rollup.items);
  const StateIcon = state === "failed" ? X : Circle;
  const stateLabel = toolRollupStateLabel(state);
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
          <ActivityKindIcon kind={rollup.kind} />
          {running ? <ToolRunningIndicator /> : null}
          {live || state === "completed" ? null : (
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
            <span className="acp-state">{stateLabel}</span>
          )}
          <ChevronDown
            className="acp-tool-rollup__chevron"
            aria-hidden="true"
            size={14}
          />
        </span>
      </summary>
      {expanded && visible ? (
        <div className="acp-tool-rollup__items">
          {rollup.items.map((item) => (
            <ToolItem
              key={item.id}
              item={item}
              compact
              allowBrowse={interactive}
              onOpenResource={interactive ? onOpenResource : undefined}
            />
          ))}
        </div>
      ) : null}
    </details>
  );
}
