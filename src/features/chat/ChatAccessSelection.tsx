import { useEffect, type ReactNode } from "react";
import {
  loadChatAccess,
  useChatAccessStore,
} from "../../app/sessionAccessActions";
import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import type { AcpSessionConfig } from "../../domain/acpSessionConfig";
import {
  sessionAccessModeAvailability,
  sessionAccessModeCandidates,
  type SessionAccessMode,
  type SessionAccessModeCapability,
} from "../../domain/sessionAccessMode";
import { CHAT_ACCESS_ICONS } from "./chatAccessIcons";
import type { AccessView } from "./ComposerAccessControl";

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

/**
 * A live chat's access: its own mode when it has one the session offers,
 * else its harness default. Loaded on demand, so the chat path carries none
 * of it.
 */
export default function ChatAccessSelection({
  bindingId,
  capabilities,
  session,
  error,
  children,
}: {
  bindingId: string;
  capabilities: readonly SessionAccessModeCapability[] | undefined;
  session: AcpSessionConfig;
  error: string | null;
  children: (view: AccessView) => ReactNode;
}) {
  const sessionId = session.sageAgentSessionId;
  const stored = useSessionAccessModeStore((state) => state.modes[bindingId]);
  const chat = useChatAccessStore((state) => state.modes[sessionId]);
  const chatError = useChatAccessStore((state) => state.error);
  const unread = chat === undefined;
  useEffect(() => {
    if (unread) void loadChatAccess(sessionId);
  }, [sessionId, unread]);
  const candidates = chatAccessCandidates(
    capabilities,
    session.modes?.availableModes.map((mode) => mode.id) ?? null,
  );
  const effective = chatAccessEffective(stored, candidates, chat);
  return children({
    candidates,
    effective,
    error: error ?? chatError,
    sessionId,
    icon: effective ? CHAT_ACCESS_ICONS[effective] : undefined,
    tone:
      effective === "fullAccess" ? " composer__access-label--full" : undefined,
  });
}
