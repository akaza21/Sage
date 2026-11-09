/**
 * Gives a session the access drafted in its chat before the session existed
 * (`chatAccessDrafts.ts`), and throws when it was not given. Drafts load with
 * the composer's access control and register here as they load; until then
 * no chat can hold one, so the send path carries none of their code.
 */
export const chatAccessDraftPort: {
  spend?: (
    chatId: string,
    bindingId: string,
    sessionId: string,
  ) => Promise<void>;
} = {};
