import type { AcpItem, ToolPresentation } from "../../../domain/acp";

export function commandRowText(summary: string): string {
  const wrapper =
    /^(?:&\s*)?("[^"]+"|'[^']+'|\S+)\s+(?:(?:-(?:noprofile|nologo|noninteractive|login)|-executionpolicy\s+\w+)\s+)*-(?:command|c|lc)\s+([\s\S]+)$/i.exec(
      summary,
    );
  if (!wrapper) return summary;
  const program = wrapper[1].replace(/^["']|["']$/g, "");
  if (
    !/(?:^|[\\/])(?:powershell|pwsh|bash|sh|zsh|dash)(?:\.exe)?$/i.test(
      program,
    ) &&
    program !== "[path]"
  )
    return summary;
  const command = wrapper[2].trim();
  return (command[0] === '"' || command[0] === "'") &&
    command.at(-1) === command[0]
    ? command.slice(1, -1)
    : command;
}

export const compactToolLabels: Partial<
  Record<ToolPresentation["kind"], string>
> = {
  execute: "Shell",
  read: "Read",
  edit: "Edit",
  delete: "Delete",
  move: "Move",
  // Integration results render bare, without a command frame.
  mcp: "",
  webSearch: "",
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
