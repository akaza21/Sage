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

export interface HarnessSignInCommand {
  bindingId?: string;
  busy: boolean;
  error: { failure: AcpCommandFailure } | null;
}
// Evidence outlives lazy UI initialization without retaining pages or errors.
type SignInEvidence = readonly [
  evidenceAt: number,
  state: "signedIn" | "signedOut" | "unavailable",
];
export const harnessSignInEvidence = new Map<string, SignInEvidence>();
export function rememberSignInEvidence(
  bindingId: string | undefined,
  evidenceAt: number | null | undefined,
  state: SignInEvidence[1],
) {
  if (!bindingId || evidenceAt == null) return;
  const previous = harnessSignInEvidence.get(bindingId);
  if (
    !previous ||
    previous[0] < evidenceAt ||
    (previous[0] === evidenceAt &&
      state === "signedOut" &&
      previous[1] !== state)
  ) {
    harnessSignInEvidence.set(bindingId, [evidenceAt, state]);
    if (harnessSignInEvidence.size > 32)
      harnessSignInEvidence.delete(harnessSignInEvidence.keys().next().value!);
    harnessSignInFailures.changed?.();
    return true;
  }
}
export const harnessSignInFailures: {
  changed?: () => void;
  command: (state: HarnessSignInCommand) => void;
  page: (page: AcpChatPage) => void;
} = {
  command({ bindingId, busy, error }: HarnessSignInCommand) {
    if (busy && error?.failure.kind === "signInRequired")
      rememberSignInEvidence(bindingId, error.failure.terminalAt, "signedOut");
  },
  page(page: AcpChatPage) {
    for (const turn of page.turns)
      if (
        turn.state === "failed" &&
        turn.terminalReason?.kind === "authRequired"
      )
        rememberSignInEvidence(
          page.sessions.find((session) => session.id === turn.sessionId)
            ?.bindingId,
          turn.terminalAt,
          "signedOut",
        );
  },
};

// Harness discovery runs before Chat; it must not retain transcript adapters.
export const acpHarnessDesktop = {
  startCodexAuth(request: CodexAuthScope): Promise<CodexAuthResponse> {
    return invokeCodexAuth("start_codex_auth", { ...request, consent: true });
  },

  cancelCodexAuth(request: CodexAuthScope): Promise<CodexAuthResponse> {
    return invokeCodexAuth("cancel_codex_auth", request);
  },

  logoutCodexAuth(request: CodexAuthScope): Promise<CodexAuthResponse> {
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
