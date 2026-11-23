import type {
  AcpMutationResult,
  AcpSessionMutationIntent,
} from "../domain/acp";
import { AcpDesktopError, invokeAcp } from "./acpDesktopCommand";
import { utf8ByteLength } from "../domain/text";
import { isRecord, safeIdentifier, safeOrdinal } from "./acpDesktopGuards";
import {
  isCommandPanel,
  isCommandSource,
  type AcpCommandPanel,
  type AcpCommandSource,
} from "../domain/acpCommand";
import { safeAcpMutationResponse } from "./acpDesktopResponses";
import {
  isCommandPlacement,
  type AcpCommandPlacement,
} from "../domain/acpCommandPlacement";

export type { AcpCommandSource };

export interface AvailableAcpCommand {
  name: string;
  description: string;
  inputHint: string | null;
  source: AcpCommandSource;
  sourceLabel?: string;
  sourceInfo?: {
    path?: string | null;
    source?: string | null;
    scope?: string | null;
    origin?: string | null;
    baseDir?: string | null;
  };
  /** Opens this panel in the chat and sends nothing. */
  panel?: AcpCommandPanel;
  aliases?: string[];
  /** Native placement; absent only in older host responses. */
  placement?: AcpCommandPlacement;
}

const MAX_COMMANDS = 512;
const MAX_COMMAND_BYTES = 256 * 1024;
const MAX_ALIASES = 8;

export interface AcpCommandList {
  revision: number;
  commands: AvailableAcpCommand[];
  omitted?: number;
  /** The binding's curated terminal handoff, decoded where it is used. */
  terminalHandoff?: unknown;
}
export interface RunAcpCommandRequest extends AcpSessionMutationIntent {
  expectedCommandsRevision: number;
  name: string;
  input?: string;
}
function invalid(): AcpDesktopError {
  return new AcpDesktopError({
    kind: "internal",
    message: "Invalid bounded command response.",
  });
}
export function decodeCommandList(value: unknown): AcpCommandList | null {
  if (
    !isRecord(value) ||
    safeOrdinal(value.revision) === null ||
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
  // Provenance fields may be null or omitted; any other key rejects the list.
  const sourceInfo = (info: unknown) =>
    isRecord(info) &&
    Object.entries(info).every(
      ([key, value]) =>
        ["path", "source", "scope", "origin", "baseDir"].includes(key) &&
        (value === null ||
          text(value, key === "path" || key === "baseDir" ? 1024 : 128)),
    );
  for (const c of value.commands) {
    const aliases: unknown =
      isRecord(c) && c.aliases !== undefined ? c.aliases : [];
    if (
      !isRecord(c) ||
      // Four required fields plus the optional fields validated below exhaust the row.
      Object.keys(c).length !==
        4 +
          +(c.sourceLabel !== undefined) +
          +(c.sourceInfo !== undefined) +
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
      (c.sourceInfo !== undefined && !sourceInfo(c.sourceInfo)) ||
      (c.panel !== undefined && !isCommandPanel(c.panel)) ||
      (c.placement !== undefined && !isCommandPlacement(c.placement))
    )
      return null;
    names.add(c.name);
    if (bytes > MAX_COMMAND_BYTES) return null;
  }
  return {
    revision: value.revision as number,
    // Every row and key is bounded above; reuse the native array without DTO copies.
    commands: value.commands as AvailableAcpCommand[],
    omitted: (value.omitted as number | undefined) ?? 0,
    ...(value.terminalHandoff === undefined
      ? {}
      : { terminalHandoff: value.terminalHandoff }),
  };
}
/** Native says one live owner's command list has a new revision; no data. */
export interface AcpCommandsChangedV1 {
  sageProjectId: string;
  sageChatId: string;
  sageAgentSessionId: string;
  runtimeEpoch: string;
  revision: number;
}
export function decodeCommandsNotice(
  value: unknown,
): AcpCommandsChangedV1 | null {
  if (
    !isRecord(value) ||
    value.schemaVersion !== 1 ||
    Object.keys(value).length !== 6
  )
    return null;
  for (const key of ["sageProjectId", "sageChatId", "sageAgentSessionId"]) {
    if (!safeIdentifier(value[key])) return null;
  }
  if (
    typeof value.runtimeEpoch !== "string" ||
    !/^[1-9][0-9]{0,19}$/u.test(value.runtimeEpoch) ||
    (safeOrdinal(value.revision) ?? 0) < 1
  )
    return null;
  const { schemaVersion: _version, ...notice } = value;
  return notice as unknown as AcpCommandsChangedV1;
}
// Separate native port, loaded only with the composer's command menu.
export const acpCommandsDesktop = {
  async subscribeCommandChanges(
    onChange: (event: AcpCommandsChangedV1) => void,
  ): Promise<() => void> {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window))
      return () => undefined;
    const { listen } = await import("@tauri-apps/api/event");
    return listen<unknown>("acp-commands-changed-v1", (event) => {
      const notice = decodeCommandsNotice(event.payload);
      if (notice) onChange(notice);
    });
  },
  async listCommands(
    request: AcpSessionMutationIntent,
  ): Promise<AcpCommandList> {
    const list = decodeCommandList(
      await invokeAcp("list_acp_commands", request),
    );
    if (!list) throw invalid();
    return list;
  },
  async runCommand(request: RunAcpCommandRequest): Promise<AcpMutationResult> {
    const result = safeAcpMutationResponse(
      await invokeAcp("run_acp_command", request),
    );
    if (!result) throw invalid();
    return result;
  },
};
