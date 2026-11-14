/**
 * Gives a session the access drafted in its chat before the session existed
 * (`chatAccessDrafts.ts`), and throws when it was not given. Drafts load with
 * the composer's access control and register here as they load; until then
 * no chat can hold one, so the send path carries none of their code.
 */
export const chatAccessDraftPort: {
  spend?: (
    chatId: string,
    session: { id: string; bindingId: string },
  ) => Promise<void>;
  /**
   * The session start's `effort` for a thinking level drafted in the chat
   * before its session existed (`chatEffortDraft.ts`), registered when the
   * draft control loads; until then no chat holds one.
   */
  effort?: (
    scope: { chatId: string },
    bindingId: string,
    modelId: string | null,
  ) => { effort?: string };
  /** Lets go of the chat's thinking level draft once a session started with it. */
  spendEffort?: (chatId: string) => void;
} = {};
