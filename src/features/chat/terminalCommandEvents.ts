import type { AcpNativeCommandControl } from "../../domain/acpCommandPlacement";
import type { TerminalHandoff } from "../../services/terminalHandoff";

/** A caught terminal-only command; `control` is set when one replaces it. */
export interface TerminalCommandRequest {
  input: string;
  name: string;
  handoff: TerminalHandoff;
  control?: AcpNativeCommandControl;
}
export type TerminalCommandEvent = CustomEvent<TerminalCommandRequest>;
/** Dispatched on the composer field; its command controller cancels it to own the choice. */
export const TERMINAL_COMMAND_EVENT = "sage-terminal-command";
