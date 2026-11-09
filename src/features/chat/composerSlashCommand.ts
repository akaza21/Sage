/**
 * The composer's `/` command menu: which text opens it and what a pick
 * writes. The harness reads a command as prompt text whose first token is
 * `/name` (claude-agent-acp turns the first text block into Claude Code's
 * slash command).
 */

/** Where the leading `/token` ends: its first whitespace, or the end. */
export function slashTokenEnd(text: string): number {
  const space = text.search(/\s/u);
  return space < 0 ? text.length : space;
}

/**
 * The command name typed so far, while the text starts with `/` and the
 * caret is inside that leading token. A null caret means the end.
 */
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
}

/** The name of a `/name …` prompt. */
export function slashCommandName(text: string): string | null {
  const match = /^\/([^\s/]+)(?:\s|$)/u.exec(text.trimStart());
  return match ? match[1]! : null;
}

/**
 * The input hint shows in place of a placeholder while the composer holds
 * exactly the picked command and nothing typed after it.
 */
export function pendingSlashCommandHint(
  text: string,
  picked: PickedSlashCommand | null,
): string | null {
  return picked?.inputHint && text === `/${picked.name} `
    ? picked.inputHint
    : null;
}
