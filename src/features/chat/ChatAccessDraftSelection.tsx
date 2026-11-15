import { useEffect, type ReactNode } from "react";
import {
  draftChatAccess,
  useChatAccessDrafts,
} from "../../app/chatAccessDrafts";
import {
  loadChatAccess,
  PERMISSIVE_ACCESS,
  useChatAccessStore,
} from "../../app/sessionAccessActions";
import { useSessionAccessSelection } from "../../app/useSessionAccessSelection";
import {
  sessionAccessModeCandidates,
  type SessionAccessMode,
  type SessionAccessModeCapability,
} from "../../domain/sessionAccessMode";
import { CHAT_ACCESS_ICONS } from "./chatAccessIcons";
import type { AccessView } from "./ComposerAccessControl";
import type { EndedSession } from "./endedSession";

/**
 * A chat's access before its next session exists: the access drafted for
 * this chat and harness, else the access its last session hands on, else
 * the harness default. Every mode the harness offers a chat can be drafted;
 * the draft is given to the session as soon as it starts, before the first
 * prompt is sent, and Auto and Full access are confirmed natively then.
 * Without a chat to hold it, nothing can be chosen.
 */
export function ChatAccessDraftSelection({
  bindingId,
  chatId,
  endedSession,
  capabilities,
  error,
  children,
}: {
  bindingId: string;
  chatId: string | undefined;
  endedSession?: EndedSession;
  capabilities: readonly SessionAccessModeCapability[] | undefined;
  error: string | null;
  children: (view: AccessView) => ReactNode;
}) {
  const { effective } = useSessionAccessSelection(bindingId, capabilities);
  // Per-chat modes are offered only where they can be drafted.
  const candidates = sessionAccessModeCandidates(capabilities, !!chatId);
  const draft = useChatAccessDrafts((state) =>
    chatId ? state.drafts[chatId] : undefined,
  );
  const drafted = candidates.find(
    (candidate) =>
      draft?.bindingId === bindingId && candidate.id === draft.mode,
  )?.id;
  const carried = useCarriedAccess(bindingId, endedSession, candidates);
  // Until the ended session's access is read, what the session gets
  // without a draft is unknown: every pick is kept as a draft.
  const base = carried === undefined ? undefined : (carried ?? effective);
  const shown = drafted ?? base ?? null;
  const carryError = useChatAccessStore((state) => state.error);
  return children({
    candidates,
    effective: shown,
    error: error ?? (carried === undefined ? carryError : null),
    icon: shown ? CHAT_ACCESS_ICONS[shown] : undefined,
    tone: shown === "fullAccess" ? " composer__access-label--full" : undefined,
    // What the session gets without a draft needs none.
    onDraft: !chatId
      ? undefined
      : (mode: SessionAccessMode) =>
          draftChatAccess(chatId, mode === base ? null : { bindingId, mode }),
  });
}

/**
 * The access the chat's ended session hands the session the next send
 * starts on its binding (`carry_chat_access`): its own mode, unless that
 * needs a confirmation; null when it hands on none, undefined until read.
 */
function useCarriedAccess(
  bindingId: string,
  endedSession: EndedSession | undefined,
  candidates: readonly SessionAccessModeCapability[],
): SessionAccessMode | null | undefined {
  const from =
    endedSession?.bindingId === bindingId ? endedSession.id : undefined;
  const mode = useChatAccessStore((state) => (from ? state.modes[from] : null));
  const unread = mode === undefined;
  useEffect(() => {
    if (from && unread) void loadChatAccess(from);
  }, [from, unread]);
  if (mode === undefined) return undefined;
  return (
    candidates.find(
      (candidate) =>
        candidate.id === mode && !PERMISSIVE_ACCESS.includes(candidate.id),
    )?.id ?? null
  );
}
