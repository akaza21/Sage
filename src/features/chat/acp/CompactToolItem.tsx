import { toolStateLabels } from "./statusSemantics";
import { compactToolLabels, toolCommandContent } from "./toolCommandContent";
import { ToolCommandFrame } from "./ToolCommandFrame";
import type { ToolItemProps } from "./ToolItem";
import { ToolItemDetails } from "./ToolItemDetails";

export function CompactToolItem({ item, onOpenResource }: ToolItemProps) {
  const presentation = item.payload.presentation;
  const kind = presentation.kind;
  const label = compactToolLabels[kind];
  if (label === undefined) return null;
  const { commandLine, output } = toolCommandContent(item);
  const exit = presentation.kind === "execute" ? presentation.exitCode : null;
  const outcome = exit
    ? `✕ Exit code ${exit}`
    : item.toolState === "completed"
      ? "✓ Success"
      : toolStateLabels[item.toolState];
  return (
    <article
      className="acp-activity-card acp-tool-compact"
      data-acp-item-id={item.id}
      data-activity-kind={kind}
      data-activity-state={item.toolState}
      role="group"
      aria-label={presentation.label}
    >
      {label ? (
        <ToolCommandFrame
          label={label}
          commandLine={
            commandLine && kind === "execute" ? `$ ${commandLine}` : commandLine
          }
          output={output}
          outcome={outcome}
          danger={Boolean(exit)}
        />
      ) : null}
      <ToolItemDetails
        item={item}
        browse={null}
        onOpenResource={onOpenResource}
      />
    </article>
  );
}
