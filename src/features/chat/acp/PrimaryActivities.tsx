import { useContext, type ReactNode, useState } from "react";
import {
  WorkedActivityVisibility,
  WorkedTurnTerminal,
} from "./WorkedActivityVisibility";
import { type AcpItem } from "../../../domain/acp";
import { type AgentArtifactOpenHandler } from "../../../domain/agentArtifact";
import {
  groupPrimaryActivities,
  liveToolItem,
  toolRollupLabel,
  toolRollupDurationLabel,
  toolRollupState,
  type ToolRollup,
} from "./toolActivityRollup";
import { CollaborationItem } from "./CollaborationItem";
import { withSubagentNames } from "./subagentActivity";
import {
  BrainCircuit,
  FilePenLine,
  FileText,
  Image,
  Command,
  Search,
  TerminalSquare,
  Workflow,
  ChevronRight,
} from "../../../ui/leanIcons";
import { ToolItem } from "./ToolItem";
import { toolStateLabels } from "./statusSemantics";

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

const activityIcons: Partial<Record<ToolRollup["kind"], typeof Workflow>> = {
  edit: FilePenLine,
  delete: FilePenLine,
  move: FilePenLine,
  execute: TerminalSquare,
  read: FileText,
  imageView: Image,
  search: Search,
  fetch: Command,
  webSearch: Command,
  mcp: Command,
  think: BrainCircuit,
};

/**
 * One activity row: a muted kind icon and label. A live row shows only the
 * shimmer; state and duration stay in the accessible name and the detail.
 */
function ToolActivityRollup({
  rollup,
  interactive,
  onOpenResource,
}: {
  rollup: ToolRollup;
  interactive: boolean;
  onOpenResource?: AgentArtifactOpenHandler;
}) {
  // A row opens only when the user opens it.
  const [expanded, setExpanded] = useState(false);
  const turnEnded = useContext(WorkedTurnTerminal);
  const label = toolRollupLabel(
    rollup.kind,
    rollup.items,
    rollup.commandActivity,
    turnEnded,
  );
  const state = toolRollupState(rollup.items);
  // The latest live child names the row (toolRollupLabel); its icon must
  // describe that same child.
  const current = liveToolItem(rollup.items, turnEnded);
  const live = Boolean(current) || (!turnEnded && state === "inProgress");
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
          setExpanded(event.currentTarget.open);
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
          <ChevronRight
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
