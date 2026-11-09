import type { ReactNode } from "react";
import {
  draftChatAccess,
  useChatAccessDrafts,
} from "../../app/chatAccessDrafts";
import { useSessionAccessSelection } from "../../app/useSessionAccessSelection";
import type {
  SessionAccessMode,
  SessionAccessModeCapability,
} from "../../domain/sessionAccessMode";
import type { AccessView } from "./ComposerAccessControl";

/**
 * A chat's access before its session exists: the access drafted for this
 * chat and harness, else the harness default. A choice is held as the
 * chat's draft and given to its session before the first prompt is sent;
 * without a chat to hold it, nothing can be chosen.
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
  const { candidates, effective } = useSessionAccessSelection(
    bindingId,
    capabilities,
  );
  const draft = useChatAccessDrafts((state) =>
    chatId ? state.drafts[chatId] : undefined,
  );
  const drafted = candidates.find(
    (candidate) =>
      draft?.bindingId === bindingId && candidate.id === draft.mode,
  )?.id;
  return children({
    candidates,
    effective: drafted ?? effective,
    error,
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
