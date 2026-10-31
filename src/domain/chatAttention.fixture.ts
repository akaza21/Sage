import type { ChatAttentionV1 } from "./chatAttention";

export function attention(
  overrides: Partial<ChatAttentionV1> = {},
): ChatAttentionV1 {
  return {
    projectId: "project-1",
    chatId: "chat-1",
    sessionId: null,
    sessionState: null,
    turnState: "completed",
    running: false,
    pendingApprovals: 0,
    pendingRequests: 0,
    activityPosition: "1",
    readPosition: null,
    terminalPosition: "1",
    terminalState: "completed",
    forcedUnreadPosition: null,
    revision: "1",
    lineage: null,
    ...overrides,
  };
}
