import type {
  AcpMutationResult,
  SetAcpSessionModeRequest,
  SetAcpSessionConfigOptionRequest,
} from "../domain/acp";
import {
  decodeSessionConfigNotice,
  type AcpSessionConfigChangedV1,
} from "../domain/acpSessionConfig";

export function sessionConfigDesktop(
  invokeMutation: <Request extends object>(
    command: string,
    request: Request,
  ) => Promise<AcpMutationResult>,
  isTauriRuntime: () => boolean,
) {
  return {
    setSessionMode(
      request: SetAcpSessionModeRequest,
    ): Promise<AcpMutationResult> {
      return invokeMutation("set_acp_session_mode", request);
    },
    setSessionConfigOption(
      request: SetAcpSessionConfigOptionRequest,
    ): Promise<AcpMutationResult> {
      return invokeMutation("set_acp_session_config_option", request);
    },
    async subscribeSessionConfigChanges(
      onChange: (event: AcpSessionConfigChangedV1) => void,
    ): Promise<() => void> {
      if (!isTauriRuntime()) return () => undefined;
      const { listen } = await import("@tauri-apps/api/event");
      return listen<unknown>("acp-session-config-changed-v1", (event) => {
        const notice = decodeSessionConfigNotice(event.payload);
        if (notice) onChange(notice);
      });
    },
  };
}
