/**
 * Drafts register here as they load, so the send path carries none of their
 * code.
 */
export const chatAccessDraftPort: {
  spend?: (
    chatId: string,
    session: { id: string; bindingId: string },
  ) => Promise<void>;
  effort?: (
    scope: { chatId: string },
    bindingId: string,
    modelId: string | null,
  ) => { effort?: string };
  /** Lets go of the chat's thinking level draft once a session started with it. */
  spendEffort?: (chatId: string) => void;
} = {};
