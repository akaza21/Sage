import type { AcpNativeCommandControl } from "../../domain/acpCommandPlacement";
import type { TerminalOnlyCommand } from "../../services/terminalHandoff";

export type TerminalCommandAction =
  | { kind: "send" }
  | { kind: "control"; control: AcpNativeCommandControl }
  | { kind: "terminal"; name: string };

/**
 * Routes a typed `/name` the session did not report. Reported commands,
 * skills and prompt templates always win; only curated terminal-only names
 * are caught, and a supported native control replaces the terminal.
 */
export function interceptTerminalCommand(
  name: string,
  reported: readonly { name: string; aliases?: readonly string[] }[],
  curated: readonly TerminalOnlyCommand[],
  controls: readonly AcpNativeCommandControl[],
): TerminalCommandAction {
  const named = (row: { name: string; aliases?: readonly string[] }) =>
    row.name === name || !!row.aliases?.includes(name);
  if (reported.some(named)) return { kind: "send" };
  const entry = curated.find(named);
  if (!entry) return { kind: "send" };
  return entry.control && controls.includes(entry.control)
    ? { kind: "control", control: entry.control }
    : { kind: "terminal", name };
}
