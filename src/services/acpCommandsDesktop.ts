import type {
  AcpMutationResult,
  AcpSessionMutationIntent,
} from "../domain/acp";
import { AcpDesktopError, invokeAcp } from "./acpDesktopCommand";
import { utf8ByteLength } from "../domain/text";
import { isRecord } from "./acpDesktopGuards";
import { safeAcpMutationResponse } from "./acpDesktopResponses";

export interface AvailableAcpCommand {
  name: string;
  description: string;
  inputHint: string | null;
}
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
    value.commands.length > 128
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
      (c.inputHint !== null && !text(c.inputHint, 256))
    )
      return null;
    names.add(c.name);
    bytes += utf8ByteLength(c.name + c.description + (c.inputHint ?? ""));
    if (bytes > 131072) return null;
    commands.push({
      name: c.name,
      description: c.description,
      inputHint: c.inputHint,
    });
  }
  return { revision: value.revision as number, commands };
}
// Separate native port: the composer follow-up can import this without adding UI here.
export const acpCommandsDesktop = {
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
