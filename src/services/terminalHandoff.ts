import type { AcpCommandList } from "./acpCommandsDesktop";

export type CaughtTerminalCommand = {
  input: string;
  name: string;
  command: string;
};
type Catcher = (caught: CaughtTerminalCommand) => boolean;
// One mounted composer field per visible chat owns its caught command.
const catchers = new Map<string, Catcher>();

export const catchTerminalCommand = (
  chatId: string,
  caught: CaughtTerminalCommand,
) => !!catchers.get(chatId)?.(caught);

export function registerTerminalCatch(chatId: string, take: Catcher) {
  catchers.set(chatId, take);
  return () => {
    if (catchers.get(chatId) === take) catchers.delete(chatId);
  };
}

/**
 * The binding's resume command when `name` is one native classifies as
 * terminal-only and the session did not report it; reported commands, skills
 * and prompt templates always win. A malformed handoff catches nothing.
 * Native bounds the names (128 rows with 8 aliases each) and the command.
 */
export function terminalOnlyCommand(
  list: AcpCommandList,
  name: string,
): string | null {
  const handoff = list.terminalHandoff as
    { command?: unknown; terminalOnly?: unknown } | undefined;
  return typeof handoff?.command === "string" &&
    Array.isArray(handoff.terminalOnly) &&
    handoff.terminalOnly.includes(name) &&
    !list.commands.some(
      (command) => command.name === name || command.aliases?.includes(name),
    )
    ? handoff.command
    : null;
}
