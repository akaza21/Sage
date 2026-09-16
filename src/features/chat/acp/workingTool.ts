import type { AcpItem } from "../../../domain/acp";
import { terminalToolStates, toolStateLabels } from "./statusSemantics";

export type WorkingToolItem = Extract<
  AcpItem,
  { kind: "toolCall" | "toolUpdateResult" }
>;

export function isActiveTool(item: AcpItem): item is WorkingToolItem {
  return (
    (item.kind === "toolCall" || item.kind === "toolUpdateResult") &&
    !terminalToolStates.has(item.toolState)
  );
}

export function isWorkingTool(item: AcpItem): item is WorkingToolItem {
  return isActiveTool(item) && item.toolState === "inProgress";
}

export function workingToolStatus(item: WorkingToolItem): string {
  const presentation = item.payload.presentation;
  if (presentation.kind === "search") {
    return `Searching… ${presentation.querySummary}`;
  }
  const detail =
    item.kind === "toolCall"
      ? item.payload.inputSummary
      : item.payload.outputSummary;
  return detail || toolStateLabels[item.toolState];
}
