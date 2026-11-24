import type { UserTerminalScope } from "../services/userTerminalDesktop";

/** `a` when it is for exactly the native scope `b`, otherwise null. */
export const forScope = <T extends UserTerminalScope>(
  a: T | null | undefined,
  b: UserTerminalScope,
) =>
  a?.projectId === b.projectId &&
  a.projectGeneration === b.projectGeneration &&
  a.chatId === b.chatId
    ? a
    : null;
