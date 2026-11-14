import type { AcpCommandPanel } from "../../domain/acpCommand";

// claude-agent-acp turns a first text block starting with `/name` into a
// Claude Code slash command.

export function slashTokenEnd(text: string): number {
  const space = text.search(/\s/u);
  return space < 0 ? text.length : space;
}

export function slashCommandQuery(
  text: string,
  caret: number | null,
): string | null {
  if (!text.startsWith("/")) return null;
  const end = slashTokenEnd(text);
  const at = caret ?? text.length;
  return at >= 1 && at <= end ? text.slice(1, end) : null;
}

export interface PickedSlashCommand {
  name: string;
  inputHint: string | null;
  panel?: AcpCommandPanel;
}

export function slashCommandName(text: string): string | null {
  const match = /^\/([^\s/]+)(?:\s|$)/u.exec(text.trimStart());
  return match ? match[1]! : null;
}

export function pendingSlashCommandHint(
  text: string,
  picked: PickedSlashCommand | null,
): string | null {
  return picked?.inputHint && text === `/${picked.name} `
    ? picked.inputHint
    : null;
}
