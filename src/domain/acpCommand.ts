import type { AcpSessionAction } from "./acpSessionAction";
import { isRecord } from "./guards";
import { utf8ByteLength } from "./text";

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

export const commandPanels = ["status", "mcp"] as const;
export type AcpCommandPanel = (typeof commandPanels)[number];

export const isCommandPanel = (value: unknown): value is AcpCommandPanel =>
  commandPanels.includes(value as AcpCommandPanel);

export interface AcpTurnCommand {
  name: string;
  source: AcpCommandSource;
}

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

const actionCommands = {
  compact: { name: "compact", source: "builtin" },
  review: { name: "review", source: "builtin" },
} as const satisfies Record<AcpSessionAction["kind"], AcpTurnCommand>;

export function turnCommand(turn: {
  command?: AcpTurnCommand;
  sessionAction?: AcpSessionAction;
}): AcpTurnCommand | undefined {
  return (
    turn.command ??
    (turn.sessionAction && actionCommands[turn.sessionAction.kind])
  );
}
