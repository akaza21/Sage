import type {
  AcpMutationResult,
  AcpSessionMutationIntent,
} from "../domain/acp";
import { AcpDesktopError, invokeAcp } from "./acpDesktopCommand";
import { utf8ByteLength } from "../domain/text";
import { isRecord } from "./acpDesktopGuards";
import { isCommandSource, type AcpCommandSource } from "../domain/acpCommand";
import { safeAcpMutationResponse } from "./acpDesktopResponses";

export type { AcpCommandSource };

/** The `/` menu's section heading for each source, in section order. */
export const commandSourceGroups = {
  builtin: "Commands",
  skill: "Skills",
  prompt: "Prompts",
  extension: "Extensions",
  mcp: "MCP prompts",
  plugin: "Plugins",
  harness: "Other commands",
} as const satisfies Record<AcpCommandSource, string>;

export interface AvailableAcpCommand {
  name: string;
  description: string;
  inputHint: string | null;
  source: AcpCommandSource;
}

// The native list's bounds.
const MAX_COMMANDS = 512;
const MAX_COMMAND_BYTES = 256 * 1024;

export interface AcpCommandList {
  revision: number;
  commands: AvailableAcpCommand[];
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
    !Number.isSafeInteger(value.revision) ||
    (value.revision as number) < 0 ||
    !Array.isArray(value.commands) ||
    value.commands.length > MAX_COMMANDS
  )
    return null;
  const names = new Set<string>();
  let bytes = 0;
  const text = (s: unknown, cap: number): s is string =>
    typeof s === "string" && !s.includes("\0") && utf8ByteLength(s) <= cap;
  const commands: AvailableAcpCommand[] = [];
  for (const c of value.commands) {
    if (
      !isRecord(c) ||
      !text(c.name, 128) ||
      !c.name ||
      /[\s/]/u.test(c.name) ||
      names.has(c.name) ||
      !text(c.description, 1024) ||
      (c.inputHint !== null && !text(c.inputHint, 256)) ||
      !isCommandSource(c.source)
    )
      return null;
    names.add(c.name);
    bytes += utf8ByteLength(c.name + c.description + (c.inputHint ?? ""));
    if (bytes > MAX_COMMAND_BYTES) return null;
    commands.push({
      name: c.name,
      description: c.description,
      inputHint: c.inputHint,
      source: c.source,
    });
  }
  return { revision: value.revision as number, commands };
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
  if (!isRecord(value) || value.schemaVersion !== 1) return null;
  for (const key of ["sageProjectId", "sageChatId", "sageAgentSessionId"]) {
    const id = value[key];
    if (typeof id !== "string" || !id || utf8ByteLength(id) > 512) return null;
  }
  if (
    typeof value.runtimeEpoch !== "string" ||
    !/^[1-9][0-9]{0,19}$/u.test(value.runtimeEpoch) ||
    !Number.isSafeInteger(value.revision) ||
    (value.revision as number) < 1
  )
    return null;
  return {
    sageProjectId: value.sageProjectId as string,
    sageChatId: value.sageChatId as string,
    sageAgentSessionId: value.sageAgentSessionId as string,
    runtimeEpoch: value.runtimeEpoch,
    revision: value.revision as number,
  };
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
