import { useState } from "react";
import {
  BrainCircuit,
  ChevronDown,
  Circle,
  Clock3,
  FilePenLine,
  FolderOpen,
  Globe2,
  LoaderCircle,
  Search,
  Terminal,
  Workflow,
  X,
} from "lucide-react";

import type { AgentArtifactOpenHandler } from "../../../domain/agentArtifact";
import { ToolItem } from "./ToolItem";
import {
  toolRollupLabel,
  toolRollupDurationLabel,
  toolRollupState,
  toolRollupStateLabel,
  type ToolRollup,
} from "./toolActivityRollup";

function ToolStateIcon({
  state,
}: {
  state: ReturnType<typeof toolRollupState>;
}) {
  if (state === "completed") return null;
  if (state === "failed") return <X aria-hidden="true" size={14} />;
  if (state === "inProgress")
    return (
      <LoaderCircle
        className="acp-tool-rollup__working"
        aria-hidden="true"
        size={14}
      />
    );
  return <Circle aria-hidden="true" size={14} />;
}

function ActivityKindIcon({ kind }: { kind: ToolRollup["kind"] }) {
  const iconProps = { "aria-hidden": true as const, size: 14 };
  switch (kind) {
    case "edit":
    case "delete":
    case "move":
      return <FilePenLine {...iconProps} data-activity-icon={kind} />;
    case "execute":
      return <Terminal {...iconProps} data-activity-icon={kind} />;
    case "read":
      return <FolderOpen {...iconProps} data-activity-icon={kind} />;
    case "search":
      return <Search {...iconProps} data-activity-icon={kind} />;
    case "fetch":
      return <Globe2 {...iconProps} data-activity-icon={kind} />;
    case "think":
      return <BrainCircuit {...iconProps} data-activity-icon={kind} />;
    default:
      return <Workflow {...iconProps} data-activity-icon={kind} />;
  }
}

export function ToolActivityRollup({
  rollup,
  interactive,
  onOpenResource,
}: {
  rollup: ToolRollup;
  interactive: boolean;
  onOpenResource?: AgentArtifactOpenHandler;
}) {
  const [expanded, setExpanded] = useState(false);
  const label = toolRollupLabel(
    rollup.kind,
    rollup.items,
    rollup.commandActivity,
  );
  const state = toolRollupState(rollup.items);
  const stateLabel = toolRollupStateLabel(state);
  const duration = toolRollupDurationLabel(rollup.items);
  const liveCommand =
    rollup.kind === "execute" &&
    rollup.items.length === 1 &&
    rollup.items[0]?.toolState === "inProgress";
  return (
    <details
      className="acp-activity-card acp-activity-card--tool-rollup acp-tool-rollup"
      data-activity-kind={rollup.kind}
      data-activity-state={state}
      onToggle={(event) => setExpanded(event.currentTarget.open)}
    >
      <summary>
        <span className="acp-tool-rollup__summary-copy">
          <ActivityKindIcon kind={rollup.kind} />
          {liveCommand ? null : <ToolStateIcon state={state} />}
          <span className="acp-tool-rollup__label">{label}</span>
        </span>
        <span className="acp-tool-rollup__meta">
          {duration ? (
            <span className="acp-tool-rollup__duration">
              <Clock3 aria-hidden="true" size={12} />
              {duration}
            </span>
          ) : null}
          {liveCommand ? null : <span className="acp-state">{stateLabel}</span>}
          <ChevronDown
            className="acp-tool-rollup__chevron"
            aria-hidden="true"
            size={14}
          />
        </span>
      </summary>
      {expanded ? (
        <div className="acp-tool-rollup__items">
          {rollup.items.map((item) => (
            <ToolItem
              key={item.id}
              item={item}
              allowBrowse={interactive}
              onOpenResource={interactive ? onOpenResource : undefined}
            />
          ))}
        </div>
      ) : null}
    </details>
  );
}
