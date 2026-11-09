import { create } from "zustand";
import type { SessionAccessMode } from "../domain/sessionAccessMode";
import { AcpDesktopError } from "../services/acpDesktopCommand";
import { chatAccessDraftPort } from "./chatAccessDraftPort";
import { applyChatAccessDraft } from "./sessionAccessActions";

/** An access choice made before a chat's session takes it. */
export interface ChatAccessDraft {
  bindingId: string;
  mode: SessionAccessMode;
}

/** Chats whose draft is held; the oldest is dropped beyond this. */
const MAX_DRAFTS = 32;

/**
 * Access chosen in a chat's composer before its session can hold it. The
 * draft is spent before the chat's next prompt is dispatched, through the
 * same native chat access commands the menu uses once the session exists.
 */
export const useChatAccessDrafts = create<{
  drafts: Readonly<Record<string, ChatAccessDraft>>;
}>(() => ({ drafts: {} }));

function withoutDraft(chatId: string) {
  const { [chatId]: _dropped, ...rest } = useChatAccessDrafts.getState().drafts;
  return rest;
}

/** Holds `draft` for the chat, or clears the chat's draft when null. */
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

/** Removes and returns the chat's draft for `bindingId`, if it has one. */
function takeChatAccessDraft(
  chatId: string,
  bindingId: string,
): SessionAccessMode | null {
  const draft = useChatAccessDrafts.getState().drafts[chatId];
  if (!draft) return null;
  useChatAccessDrafts.setState({ drafts: withoutDraft(chatId) });
  return draft.bindingId === bindingId ? draft.mode : null;
}

/**
 * Gives the chat's new `session` the chat's draft for its binding before a
 * prompt is dispatched, and throws when it was not given, so nothing is
 * sent. The draft is spent either way: once the session exists, its access
 * menu shows and changes what it holds.
 */
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
