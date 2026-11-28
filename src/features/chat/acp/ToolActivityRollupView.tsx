import { useContext, useState } from "react";
import { type AgentArtifactOpenHandler } from "../../../domain/agentArtifact";
import { ChevronRight } from "../../../ui/leanIcons";
import { ToolItem } from "./ToolItem";
import { WorkedTurnTerminal } from "./WorkedTurnTerminal";
import { toolStateLabels } from "./statusSemantics";
import {
  liveToolItem,
  liveToolProgress,
  namesCategory,
  toolItemLabel,
  toolRollupDurationLabel,
  toolRollupLabel,
  toolRollupState,
  type ToolRollup,
} from "./toolActivityRollup";

export function ToolActivityRollup({
  rollup,
  interactive,
  onOpenResource,
  nested = false,
}: {
  rollup: ToolRollup;
  interactive: boolean;
  onOpenResource?: AgentArtifactOpenHandler;
  /** A row inside an opened Worked-for row names its own file or command. */
  nested?: boolean;
}) {
  const [expanded, setExpanded] = useState(false);
  const turnEnded = useContext(WorkedTurnTerminal);
  const current = liveToolItem(rollup.items, turnEnded);
  const label =
    nested && rollup.items.length === 1
      ? toolItemLabel(
          rollup.items[0]!,
          Boolean(current),
          rollup.commandActivity === "declaredTest",
        )
      : toolRollupLabel(
          rollup.kind,
          rollup.items,
          rollup.commandActivity,
          turnEnded,
        );
  const state = toolRollupState(rollup.items);
  const live = Boolean(current) || (!turnEnded && state === "inProgress");
  const duration = toolRollupDurationLabel(rollup.items);
  const progress = liveToolProgress(current);
  return (
    <details
      className="acp-activity acp-tool-rollup"
      data-activity-kind={rollup.kind}
      data-activity-state={state}
      open={expanded}
      onToggle={(event) => {
        if (event.currentTarget.open !== expanded)
          setExpanded(event.currentTarget.open);
      }}
    >
      <summary
        aria-label={[label, progress, toolStateLabels[state], duration]
          .filter(Boolean)
          .join(", ")}
      >
        <span className="acp-tool-rollup__summary-copy">
          <span
            className={`acp-tool-rollup__label${live ? " acp-live-shimmer" : ""}`}
          >
            {label}
          </span>
          {progress ? (
            <span className="acp-tool-rollup__progress">{progress}</span>
          ) : null}
        </span>
        <ChevronRight
          className="acp-tool-rollup__chevron"
          aria-hidden="true"
          size={14}
        />
      </summary>
      {expanded ? (
        <div className="acp-tool-rollup__items stagger-reveal">
          {rollup.items.map((item) => (
            <div key={item.id} className="acp-tool-rollup__row">
              {rollup.kind !== "subagent" &&
              (rollup.items.length > 1 ||
                (!nested && namesCategory(rollup.kind))) ? (
                <ToolActivityRollup
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
                  nested
                />
              ) : (
                <ToolItem
                  item={item}
                  compact
                  allowBrowse={interactive}
                  onOpenResource={interactive ? onOpenResource : undefined}
                />
              )}
            </div>
          ))}
        </div>
      ) : null}
    </details>
  );
}
