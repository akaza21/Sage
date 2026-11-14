import {
  sessionAccessModeAvailability,
  sessionAccessModeCandidates,
  type SessionAccessMode,
  type SessionAccessModeCapability,
} from "../../domain/sessionAccessMode";

/**
 * The modes a live chat can take. A mode carried by a harness session mode
 * is offered only while the session advertises it (`advertised`, null until
 * reported), and Full access only once the session reports its modes.
 */
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

/**
 * The modes a chat whose session is not live can take: every mode the
 * harness offers a chat, since the session that runs its next prompt
 * reports its own session modes only once it starts.
 */
export function heldChatAccessCandidates(
  capabilities: readonly SessionAccessModeCapability[] | undefined,
) {
  return sessionAccessModeCandidates(capabilities, true);
}

/**
 * Whether the chat's own mode is one its harness cannot carry now: not
 * offered at all, or a session mode the live session (`advertised`, null
 * until reported) does not advertise. Sending then fails, so the composer
 * says so rather than show another mode as the chat's.
 */
export function chatAccessUnavailable(
  capabilities: readonly SessionAccessModeCapability[] | undefined,
  advertised: readonly string[] | null,
  chat: SessionAccessMode | null | undefined,
) {
  if (!chat) return false;
  const offered = heldChatAccessCandidates(capabilities).find(
    (capability) => capability.id === chat,
  );
  return (
    !offered ||
    (advertised !== null &&
      offered.sessionModeId !== undefined &&
      !advertised.includes(offered.sessionModeId))
  );
}

export const CHAT_ACCESS_UNAVAILABLE =
  "This chat's access mode is not available from this harness now. Choose another one before sending.";

/** The mode the chat's next prompt runs under: its own, else the default. */
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
