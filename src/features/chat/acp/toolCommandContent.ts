import type { AcpItem, ToolPresentation } from "../../../domain/acp";

export const compactToolLabels: Partial<
  Record<ToolPresentation["kind"], string>
> = {
  execute: "Shell",
  read: "Read",
  edit: "Edit",
  delete: "Delete",
  move: "Move",
};

export function toolCommandContent(
  item: Extract<AcpItem, { kind: "toolCall" }>,
) {
  const { inputSummary, outputSummary, presentation } = item.payload;
  return {
    commandLine: inputSummary?.trim()
      ? inputSummary
      : presentation.kind === "execute"
        ? presentation.summary?.trim() || null
        : null,
    output:
      outputSummary ??
      (presentation.kind === "execute" ? presentation.outputTail : null),
  };
}
