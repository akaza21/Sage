import type { AcpSessionMutationIntent } from "../domain/acp";
import { AcpDesktopError, invokeAcp } from "./acpDesktopCommand";
import { utf8ByteLength } from "../domain/text";
import { isCommandPanel, isCommandSource } from "../domain/acpCommand";
import { isCommandPlacement } from "../domain/acpCommandPlacement";
import { isRecord, safeOrdinal } from "./acpDesktopGuards";
import {
  acpCommandsDesktop,
  type AcpCommandList,
  type AvailableAcpCommand,
} from "./acpCommandsDesktop";

export interface AcpCommandCatalog extends AcpCommandList {
  pending?: boolean;
}

// Refresh state belongs to the lazy command menu; first-paint callers only need command authority.
export const acpCommandCatalogDesktop = {
  subscribeCommandChanges: acpCommandsDesktop.subscribeCommandChanges,
  async refreshCommands(request: AcpSessionMutationIntent): Promise<void> {
    await invokeAcp("refresh_acp_commands", request);
  },
  async listCommands(
    request: AcpSessionMutationIntent,
  ): Promise<AcpCommandCatalog> {
    const raw = await invokeAcp("list_acp_commands", request);
    const list = decodeCommandList(raw);
    if (!list) {
      throw new AcpDesktopError({
        kind: "internal",
        message: "Invalid bounded command response.",
      });
    }
    return list;
  },
};

const MAX_COMMANDS = 512;
const MAX_COMMAND_BYTES = 256 * 1024;
const MAX_ALIASES = 8;

export function decodeCommandList(value: unknown): AcpCommandCatalog | null {
  if (
    !isRecord(value) ||
    safeOrdinal(value.revision) === null ||
    (value.pending !== undefined && typeof value.pending !== "boolean") ||
    (value.omitted !== undefined &&
      (safeOrdinal(value.omitted) === null ||
        (value.omitted as number) > 0xffff_ffff)) ||
    !Array.isArray(value.commands) ||
    value.commands.length > MAX_COMMANDS
  )
    return null;
  const names = new Set<string>();
  let bytes = 0;
  const text = (s: unknown, cap: number): s is string => {
    if (typeof s !== "string" || s.includes("\0")) return false;
    const size = utf8ByteLength(s);
    bytes += size;
    return size <= cap;
  };
  const commandName = (s: unknown): s is string =>
    text(s, 128) && s !== "" && !/[\s/]/u.test(s);
  for (const c of value.commands) {
    const aliases: unknown =
      isRecord(c) && c.aliases !== undefined ? c.aliases : [];
    if (
      !isRecord(c) ||
      // Four required fields plus the optional fields validated below exhaust the row.
      Object.keys(c).length !==
        4 +
          +(c.sourceLabel !== undefined) +
          +(c.panel !== undefined) +
          +(c.aliases !== undefined) +
          +(c.placement !== undefined) ||
      !commandName(c.name) ||
      !Array.isArray(aliases) ||
      aliases.length > MAX_ALIASES ||
      !aliases.every(commandName) ||
      names.has(c.name) ||
      !text(c.description, 1024) ||
      (c.inputHint !== null && !text(c.inputHint, 256)) ||
      !isCommandSource(c.source) ||
      (c.sourceLabel !== undefined && !text(c.sourceLabel, 128)) ||
      (c.panel !== undefined && !isCommandPanel(c.panel)) ||
      (c.placement !== undefined && !isCommandPlacement(c.placement))
    )
      return null;
    names.add(c.name);
    if (bytes > MAX_COMMAND_BYTES) return null;
  }
  return {
    revision: value.revision as number,
    pending: value.pending === true,
    // Every row and key is bounded above; reuse the native array without DTO copies.
    commands: value.commands as AvailableAcpCommand[],
    omitted: (value.omitted as number | undefined) ?? 0,
  };
}
