import type {
  AcpMutationResult,
  AcpSessionMutationIntent,
} from "../domain/acp";
import { AcpDesktopError, invokeAcp } from "./acpDesktopCommand";
import { isRecord, safeIdentifier, safeOrdinal } from "./acpDesktopGuards";
import {
  type AcpCommandPanel,
  type AcpCommandSource,
} from "../domain/acpCommand";
import { safeAcpMutationResponse } from "./acpDesktopResponses";
import type { AcpCommandPlacement } from "../domain/acpCommandPlacement";

export type { AcpCommandSource };

export interface AvailableAcpCommand {
  name: string;
  description: string;
  inputHint: string | null;
  source: AcpCommandSource;
  sourceLabel?: string;
  /** Opens this panel in the chat and sends nothing. */
  panel?: AcpCommandPanel;
  aliases?: string[];
  /** Native placement; absent only in older host responses. */
  placement?: AcpCommandPlacement;
}

export interface AcpCommandList {
  revision: number;
  commands: AvailableAcpCommand[];
  omitted?: number;
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
    const { acpCommandCatalogDesktop } =
      await import("./acpCommandCatalogDesktop");
    return acpCommandCatalogDesktop.listCommands(request);
  },
  async runCommand(request: RunAcpCommandRequest): Promise<AcpMutationResult> {
    const result = safeAcpMutationResponse(
      await invokeAcp("run_acp_command", request),
    );
    if (!result) throw invalid();
    return result;
  },
};
