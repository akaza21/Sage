import { isRecord } from "../domain/guards";
import { utf8ByteLength } from "../domain/text";
import type { AcpNativeCommandControl } from "../domain/acpCommandPlacement";

/** A command only the harness's own terminal runs, from curated metadata. */
export interface TerminalOnlyCommand {
  name: string;
  aliases: readonly string[];
  /** The control that replaces it, while the handoff's `controls` lists it. */
  control?: AcpNativeCommandControl;
}

/** The binding's terminal handoff, projected with its command list. */
export interface TerminalHandoff {
  label: string;
  /** Prefilled for the user, never run: the resume command when known. */
  command: string;
  terminalOnly: readonly TerminalOnlyCommand[];
  controls: readonly AcpNativeCommandControl[];
}

const MAX_ROWS = 128;
const MAX_ALIASES = 8;
/** The terminal dock's input cap. */
const MAX_COMMAND_BYTES = 4_096;

const text = (value: unknown, cap: number): value is string =>
  typeof value === "string" &&
  !!value &&
  utf8ByteLength(value) <= cap &&
  !/[\p{Cc}\p{Cf}]/u.test(value);
const name = (value: unknown): value is string =>
  text(value, 128) && !/[\s/]/u.test(value);
// An unknown control opens nothing here, so the terminal choice stays.
const control = (value: unknown): value is AcpNativeCommandControl =>
  typeof value === "string" && /^[a-z][A-Za-z]{0,63}$/u.test(value);

/** Fails closed: a malformed handoff rejects the whole list. */
export function decodeTerminalHandoff(value: unknown): TerminalHandoff | null {
  if (
    !isRecord(value) ||
    Object.keys(value).length !== 4 ||
    !text(value.label, 128) ||
    !text(value.command, MAX_COMMAND_BYTES) ||
    !Array.isArray(value.terminalOnly) ||
    value.terminalOnly.length > MAX_ROWS ||
    !Array.isArray(value.controls) ||
    value.controls.length > 32 ||
    !value.controls.every(control) ||
    new Set(value.controls).size !== value.controls.length
  )
    return null;
  const names = new Set<string>();
  for (const row of value.terminalOnly as unknown[]) {
    if (
      !isRecord(row) ||
      Object.keys(row).length !== (row.control === undefined ? 2 : 3) ||
      !name(row.name) ||
      !Array.isArray(row.aliases) ||
      row.aliases.length > MAX_ALIASES ||
      !row.aliases.every(name) ||
      (row.control !== undefined && !control(row.control))
    )
      return null;
    for (const entry of [row.name, ...row.aliases]) {
      if (names.has(entry)) return null;
      names.add(entry);
    }
  }
  // Every row and key is bounded above; reuse the native value without copies.
  return value as unknown as TerminalHandoff;
}
