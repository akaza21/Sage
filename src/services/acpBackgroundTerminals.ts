import type { SafeResourceLocation } from "../domain/acpProjection";
import type { AcpUsageRequest } from "../domain/acpUsage";
import { AcpDesktopError, invokeAcp } from "./acpDesktopCommand";
import { isRecord, safeText } from "./acpDesktopGuards";
import { safeResourceLocation } from "./acpDesktopResponsePayloads";

/** Rust `MAX_BACKGROUND_TERMINALS`. */
const MAX_TERMINALS = 32;

export interface BackgroundTerminal {
  /** Opaque; sent back to stop this terminal. */
  handle: string;
  command: string;
  cwd: SafeResourceLocation | null;
}

export function decodeBackgroundTerminals(
  value: unknown,
): BackgroundTerminal[] | null {
  if (!Array.isArray(value) || value.length > MAX_TERMINALS) return null;
  const terminals: BackgroundTerminal[] = [];
  for (const entry of value) {
    if (!isRecord(entry)) return null;
    const handle = safeText(entry.handle, 128);
    const command = safeText(entry.command, 4_096);
    const cwd = entry.cwd == null ? null : safeResourceLocation(entry.cwd);
    if (!handle || command === null || (entry.cwd != null && !cwd)) return null;
    terminals.push({ handle, command, cwd });
  }
  return terminals;
}

async function list(runtime: AcpUsageRequest) {
  const decoded = decodeBackgroundTerminals(
    await invokeAcp("get_background_terminals", runtime),
  );
  if (!decoded)
    throw new AcpDesktopError({
      kind: "internal",
      message: "Background terminals returned an invalid bounded response.",
    });
  return decoded;
}

export const acpBackgroundTerminals = {
  list,
  /** Stops one terminal, or every one when `handle` is null. */
  stop: async (runtime: AcpUsageRequest, handle: string | null) => {
    await invokeAcp("stop_background_terminals", { runtime, handle });
  },
};
