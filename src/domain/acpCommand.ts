import type { AcpSessionAction } from "./acpSessionAction";
import { isRecord } from "./guards";
import { utf8ByteLength } from "./text";

/** Where a command comes from, as its harness reports it. */
export const commandSources = [
  "builtin",
  "skill",
  "prompt",
  "extension",
  "mcp",
  "plugin",
  "harness",
] as const;
export type AcpCommandSource = (typeof commandSources)[number];

export const isCommandSource = (value: unknown): value is AcpCommandSource =>
  commandSources.includes(value as AcpCommandSource);

/** The listed command a Turn ran. */
export interface AcpTurnCommand {
  name: string;
  source: AcpCommandSource;
}

/** A stored command mark, or null for one this build cannot show. */
export function decodeTurnCommand(value: unknown): AcpTurnCommand | null {
  if (!isRecord(value) || Object.keys(value).length !== 2) return null;
  const { name, source } = value;
  return typeof name === "string" &&
    /^[^\s/]+$/u.test(name) &&
    utf8ByteLength(name) <= 128 &&
    isCommandSource(source)
    ? { name, source }
    : null;
}

/** Each session action, as the harness's own command it runs as. */
const actionCommands = {
  compact: { name: "compact", source: "builtin" },
  review: { name: "review", source: "builtin" },
} as const satisfies Record<AcpSessionAction["kind"], AcpTurnCommand>;

/** The command a Turn's prompt ran: a listed command or a session action. */
export function turnCommand(turn: {
  command?: AcpTurnCommand;
  sessionAction?: AcpSessionAction;
}): AcpTurnCommand | undefined {
  return (
    turn.command ??
    (turn.sessionAction && actionCommands[turn.sessionAction.kind])
  );
}
