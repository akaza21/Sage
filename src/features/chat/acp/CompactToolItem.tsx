import { ToolItemDetails } from "./ToolItemDetails";
import type { ToolItemProps } from "./ToolItem";
import { ToolCommandFrame } from "./ToolCommandFrame";
import { compactToolLabels, toolCommandContent } from "./toolCommandContent";
import { toolStateLabels } from "./statusSemantics";

export function CompactToolItem({ item, onOpenResource }: ToolItemProps) {
  const presentation = item.payload.presentation;
  const kind = presentation.kind;
  const label = compactToolLabels[kind];
  if (!label) return null;
  const { commandLine, output } = toolCommandContent(item);
  const outcome = [
    "failed",
    "cancelled",
    "declined",
    "waitingForApproval",
  ].includes(item.toolState)
    ? toolStateLabels[item.toolState]
    : null;
  return (
    <article
      className="acp-activity-card acp-tool-compact"
      data-acp-item-id={item.id}
      data-activity-kind={kind}
      data-activity-state={item.toolState}
      role="group"
      aria-label={presentation.label}
    >
      <ToolCommandFrame
        label={label}
        commandLine={
          commandLine && kind === "execute" ? `$ ${commandLine}` : commandLine
        }
        output={output}
        outcome={outcome}
      />
      <ToolItemDetails
        item={item}
        browse={null}
        onOpenResource={onOpenResource}
      />
    </article>
  );
}
