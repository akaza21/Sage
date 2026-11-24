import type { UserTerminalScope } from "../services/userTerminalDesktop";

/** Whether a request or a later selection is for exactly this native scope. */
export const sameScope = (
  a: UserTerminalScope | null | undefined,
  b: UserTerminalScope,
) =>
  a?.projectId === b.projectId &&
  a.projectGeneration === b.projectGeneration &&
  a.chatId === b.chatId;
