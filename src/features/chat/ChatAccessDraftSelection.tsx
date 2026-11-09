import type { ReactNode } from "react";
import {
  draftChatAccess,
  useChatAccessDrafts,
} from "../../app/chatAccessDrafts";
import { useSessionAccessSelection } from "../../app/useSessionAccessSelection";
import {
  sessionAccessModeCandidates,
  type SessionAccessMode,
  type SessionAccessModeCapability,
} from "../../domain/sessionAccessMode";
import { CHAT_ACCESS_ICONS } from "./chatAccessIcons";
import type { AccessView } from "./ComposerAccessControl";

/**
 * A chat's access before its session exists: the access drafted for this
 * chat and harness, else the harness default. Every mode the harness
 * offers a chat can be drafted; the draft is given to the session as soon as
 * it starts, before the first prompt is sent, and Auto and Full access are
 * confirmed natively then. Without a chat to hold it, nothing can be chosen.
 */
export function ChatAccessDraftSelection({
  bindingId,
  chatId,
  capabilities,
  error,
  children,
}: {
  bindingId: string;
  chatId: string | undefined;
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
  const shown = drafted ?? effective;
  return children({
    candidates,
    effective: shown,
    error,
    icon: shown ? CHAT_ACCESS_ICONS[shown] : undefined,
    tone: shown === "fullAccess" ? " composer__access-label--full" : undefined,
    // The harness default needs no draft.
    onDraft: !chatId
      ? undefined
      : (mode: SessionAccessMode) =>
          draftChatAccess(
            chatId,
            mode === effective ? null : { bindingId, mode },
          ),
  });
}
