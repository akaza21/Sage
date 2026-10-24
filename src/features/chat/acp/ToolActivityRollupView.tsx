import { useContext, useState } from "react";
import {
  BrainCircuit,
  FilePenLine,
  FileText,
  Image,
  Plug,
  Search,
  Terminal,
  Workflow,
  ChevronDown,
} from "../../../ui/leanIcons";

import type { AgentArtifactOpenHandler } from "../../../domain/agentArtifact";
import { ToolItem } from "./ToolItem";
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
  read: FileText,
  imageView: Image,
  search: Search,
  fetch: Plug,
  webSearch: Plug,
  mcp: Plug,
  think: BrainCircuit,
};

/**
 * One activity row: a muted kind icon and label. A live row shows only the
 * shimmer; state and duration stay in the accessible name and the detail.
 */
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
  // The latest live child names the row (toolRollupLabel); its icon must
  // describe that same child.
  const current = [...rollup.items]
    .reverse()
    .find((item) => item.toolState === "inProgress");
  const live = Boolean(current) || state === "inProgress";
  const iconKind = (current ?? rollup.items[0])?.payload.presentation.kind;
  const ActivityIcon = (iconKind && activityIcons[iconKind]) || Workflow;
  const duration = toolRollupDurationLabel(rollup.items);
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
      <summary
        aria-label={[label, toolStateLabels[state], duration]
          .filter(Boolean)
          .join(", ")}
      >
        <span className="acp-tool-rollup__summary-copy">
          <ActivityIcon
            aria-hidden="true"
            size={14}
            data-activity-icon={iconKind}
          />
          <span
            className={`acp-tool-rollup__label${live ? " acp-live-shimmer" : ""}`}
          >
            {label}
          </span>
        </span>
        <span className="acp-tool-rollup__meta">
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
            rollup.items.length > 1 && rollup.kind !== "subagent" ? (
              <ToolActivityRollup
                key={item.id}
                rollup={{
                  kind: item.payload.presentation.kind,
                  commandActivity:
                    item.payload.presentation.kind === "execute"
                      ? item.payload.presentation.activity
                      : "ordinary",
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
