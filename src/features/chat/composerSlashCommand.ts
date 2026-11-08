/**
 * The composer's `/` command menu: which text opens it and what a pick
 * writes. The harness reads a command as prompt text whose first token is
 * `/name` (claude-agent-acp turns the first text block into Claude Code's
 * slash command), so a pick writes `/name ` and the normal send path carries
 * it.
 */

/** The text typed after `/` while the composer holds only a command name. */
export function slashCommandQuery(text: string): string | null {
  return /^\/\S*$/u.test(text) ? text.slice(1) : null;
}

export function slashCommandText(name: string): string {
  return `/${name} `;
}

export interface PickedSlashCommand {
  name: string;
  inputHint: string | null;
}

/**
 * The input hint shows in place of a placeholder while the composer holds
 * exactly the picked command and nothing typed after it.
 */
export function pendingSlashCommandHint(
  text: string,
  picked: PickedSlashCommand | null,
): string | null {
  return picked?.inputHint && text === slashCommandText(picked.name)
    ? picked.inputHint
    : null;
}
