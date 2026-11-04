import { safeCodexDiscovery } from "./acpHarnessResponses";
import type {
  CodexAuthResponse,
  CodexAuthScope,
  CodexDiscoveryResponse,
  GetCodexDiscoveryRequest,
} from "../domain/acp";
import { AcpDesktopError, invokeAcp } from "./acpDesktopCommand";
import {
  isOneOf,
  isRecord,
  safeCounter,
  safeIdentifier,
} from "./acpDesktopGuards";

// Harness discovery runs before Chat; it must not retain transcript adapters.
function safeCodexAuth(value: unknown): CodexAuthResponse | null {
  if (!isRecord(value)) return null;
  const projectId = safeIdentifier(value.projectId);
  const projectGeneration = safeCounter(value.projectGeneration, true);
  const bindingId = safeIdentifier(value.bindingId);
  const revision = safeCounter(value.revision, false);
  if (
    !projectId ||
    !projectGeneration ||
    !bindingId ||
    !revision ||
    !isOneOf(value.phase, [
      "signedOut",
      "waiting",
      "recovery",
      "signedIn",
    ] as const)
  ) {
    return null;
  }
  return {
    projectId,
    projectGeneration,
    bindingId,
    phase: value.phase,
    revision,
  } as CodexAuthResponse;
}

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
async function invokeCodexAuth(
  command: string,
  request: CodexAuthScope | (CodexAuthScope & { consent: true }),
): Promise<CodexAuthResponse> {
  const response = await invokeAcp<unknown, typeof request>(command, request);
  const safe = safeCodexAuth(response);
  if (!safe) {
    throw new AcpDesktopError({
      kind: "internal",
      message: "Codex authentication returned an invalid bounded response.",
    });
  }
  return safe;
}
