import { useEffect, type ReactNode } from "react";
import {
  loadChatAccess,
  useChatAccessStore,
} from "../../app/sessionAccessActions";
import { useSessionAccessModeStore } from "../../app/sessionAccessModeStore";
import type { AcpSessionConfig } from "../../domain/acpSessionConfig";
import type { SessionAccessModeCapability } from "../../domain/sessionAccessMode";
import { chatAccessCandidates, chatAccessEffective } from "./chatAccessMode";
import { CHAT_ACCESS_ICONS } from "./chatAccessIcons";
import type {
  AccessView,
  ComposerAccessControlProps,
} from "./ComposerAccessControl";
import { ChatAccessDraftSelection } from "./ChatAccessDraftSelection";

interface AccessSelectionProps {
  bindingId: string;
  capabilities: readonly SessionAccessModeCapability[] | undefined;
  error: string | null;
  children: (view: AccessView) => ReactNode;
}

/** A chat's access: its live session's, or the access drafted for it. */
export function ChatAccessView({
  sessionConfig: session,
  chatId,
  ...props
}: AccessSelectionProps &
  Pick<ComposerAccessControlProps, "sessionConfig" | "chatId">) {
  // A removed session is past; its chat drafts like one not yet started.
  return session && !session.removed ? (
    <LiveChatAccess session={session} {...props} />
  ) : (
    <ChatAccessDraftSelection chatId={chatId} {...props} />
  );
}

/**
 * A live chat's access: its own mode when it has one the session offers,
 * else its harness default.
 */
function LiveChatAccess({
  bindingId,
  capabilities,
  session,
  error,
  children,
}: AccessSelectionProps & { session: AcpSessionConfig }) {
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
    defaultMode: chatAccessEffective(stored, candidates, null),
    error: error ?? chatError,
    sessionId,
    icon: effective ? CHAT_ACCESS_ICONS[effective] : undefined,
    tone:
      effective === "fullAccess" ? " composer__access-label--full" : undefined,
  });
}
