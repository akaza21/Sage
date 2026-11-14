import {
  sessionAccessModeAvailability,
  sessionAccessModeCandidates,
  type SessionAccessMode,
  type SessionAccessModeCapability,
} from "../../domain/sessionAccessMode";

export function chatAccessCandidates(
  capabilities: readonly SessionAccessModeCapability[] | undefined,
  advertised: readonly string[] | null,
) {
  return sessionAccessModeCandidates(capabilities, advertised !== null).filter(
    (capability) =>
      capability.sessionModeId === undefined ||
      advertised === null ||
      advertised.includes(capability.sessionModeId),
  );
}

export function chatAccessEffective(
  stored: SessionAccessMode | undefined,
  candidates: readonly SessionAccessModeCapability[],
  chat: SessionAccessMode | null | undefined,
): SessionAccessMode | null {
  return (
    candidates.find((candidate) => candidate.id === chat)?.id ??
    sessionAccessModeAvailability(stored, candidates).effective
  );
}
