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
    onDraft: !chatId
      ? undefined
      : (mode: SessionAccessMode) =>
          draftChatAccess(
            chatId,
            mode === effective ? null : { bindingId, mode },
          ),
  });
}
