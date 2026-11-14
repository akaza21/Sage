import { create } from "zustand";
import type { SessionAccessMode } from "../domain/sessionAccessMode";
import { AcpDesktopError } from "../services/acpDesktopCommand";
import { chatAccessDraftPort } from "./chatAccessDraftPort";
import { applyChatAccessDraft } from "./sessionAccessActions";

export interface ChatAccessDraft {
  bindingId: string;
  mode: SessionAccessMode;
}

const MAX_DRAFTS = 32;

export const useChatAccessDrafts = create<{
  drafts: Readonly<Record<string, ChatAccessDraft>>;
}>(() => ({ drafts: {} }));

function withoutDraft(chatId: string) {
  const { [chatId]: _dropped, ...rest } = useChatAccessDrafts.getState().drafts;
  return rest;
}

export function draftChatAccess(chatId: string, draft: ChatAccessDraft | null) {
  const rest = withoutDraft(chatId);
  useChatAccessDrafts.setState({
    drafts: draft
      ? Object.fromEntries(
          [...Object.entries(rest), [chatId, draft] as const].slice(
            -MAX_DRAFTS,
          ),
        )
      : rest,
  });
}

function takeChatAccessDraft(
  chatId: string,
  bindingId: string,
): SessionAccessMode | null {
  const draft = useChatAccessDrafts.getState().drafts[chatId];
  if (!draft) return null;
  useChatAccessDrafts.setState({ drafts: withoutDraft(chatId) });
  return draft.bindingId === bindingId ? draft.mode : null;
}

export async function spendChatAccessDraft(
  chatId: string,
  session: { id: string; bindingId: string },
): Promise<void> {
  const mode = takeChatAccessDraft(chatId, session.bindingId);
  if (!mode || (await applyChatAccessDraft(session.id, mode))) return;
  throw new AcpDesktopError({
    kind: "invalidRequest",
    message:
      "The chosen access was not given to this chat, so nothing was sent. Check its access and send again.",
  });
}

// A chat can hold a draft only once this module has loaded, so the send path
// reaches drafts through the port from then on.
chatAccessDraftPort.spend = spendChatAccessDraft;
