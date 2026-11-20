import type { CodexAuthResponse } from "../domain/acp";
import { AcpDesktopError } from "./acpDesktopCommand";
import {
  isOneOf,
  isRecord,
  safeCounter,
  safeIdentifier,
} from "./acpDesktopGuards";

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

export function decodeCodexAuth(value: unknown): CodexAuthResponse {
  const safe = safeCodexAuth(value);
  if (!safe)
    throw new AcpDesktopError({
      kind: "internal",
      message: "Codex authentication returned an invalid bounded response.",
    });
  return safe;
}
