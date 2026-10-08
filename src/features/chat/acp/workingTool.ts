import type { AcpItem } from "../../../domain/acp";
import { terminalToolStates, toolStateLabels } from "./statusSemantics";

export type WorkingToolItem = Extract<AcpItem, { kind: "toolCall" }>;

function isActiveTool(item: AcpItem): item is WorkingToolItem {
  return item.kind === "toolCall" && !terminalToolStates.has(item.toolState);
}

export function isWorkingTool(item: AcpItem): item is WorkingToolItem {
  return isActiveTool(item) && item.toolState === "inProgress";
}

export function workingToolStatus(item: WorkingToolItem): string {
  const presentation = item.payload.presentation;
  if (presentation.kind === "search") {
    return `Searching… ${presentation.querySummary}`;
  }
  const detail = item.payload.inputSummary;
  return detail || toolStateLabels[item.toolState];
}
