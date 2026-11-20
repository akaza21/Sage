import { safeCodexDiscovery } from "./acpHarnessResponses";
import type {
  AcpChatPage,
  AcpCommandFailure,
  CodexAuthResponse,
  CodexAuthScope,
  CodexDiscoveryResponse,
  GetCodexDiscoveryRequest,
} from "../domain/acp";
import { AcpDesktopError, invokeAcp } from "./acpDesktopCommand";

// The account cache connects on first use. Until then there is no cached fact
// to supersede. This port retains no events or views.
export interface HarnessSignInCommand {
  bindingId?: string;
  busy: boolean;
  error: { failure: AcpCommandFailure } | null;
}
export const harnessSignInFailures: {
  command?: (state: HarnessSignInCommand) => void;
  page?: (previous: AcpChatPage, page: AcpChatPage) => void;
} = {};

// Harness discovery runs before Chat; it must not retain transcript adapters.
export const acpHarnessDesktop = {
  async startCodexAuth(request: CodexAuthScope): Promise<CodexAuthResponse> {
    return invokeCodexAuth("start_codex_auth", { ...request, consent: true });
  },

  async cancelCodexAuth(request: CodexAuthScope): Promise<CodexAuthResponse> {
    return invokeCodexAuth("cancel_codex_auth", request);
  },

  async logoutCodexAuth(request: CodexAuthScope): Promise<CodexAuthResponse> {
    return invokeCodexAuth("logout_codex_auth", request);
  },

  async discoverCodex(
    request: GetCodexDiscoveryRequest,
  ): Promise<CodexDiscoveryResponse> {
    const response = await invokeAcp<unknown, GetCodexDiscoveryRequest>(
      "get_codex_discovery",
      request,
    );
    const safe = safeCodexDiscovery(response);
    if (!safe) {
      throw new AcpDesktopError({
        kind: "internal",
        message: "Codex discovery returned an invalid bounded response.",
      });
    }
    return safe;
  },
};
interface CodexAuthRequests {
  get_codex_auth: CodexAuthScope;
  start_codex_auth: CodexAuthScope & { consent: true };
  cancel_codex_auth: CodexAuthScope;
  logout_codex_auth: CodexAuthScope;
  login_codex_api_key: CodexAuthScope & { apiKey: string };
}

export async function invokeCodexAuth<C extends keyof CodexAuthRequests>(
  command: C,
  request: CodexAuthRequests[C],
): Promise<CodexAuthResponse> {
  const [response, { decodeCodexAuth }] = await Promise.all([
    invokeAcp<unknown, typeof request>(command, request),
    import("./acpHarnessAuthResponse"),
  ]);
  return decodeCodexAuth(response);
}
