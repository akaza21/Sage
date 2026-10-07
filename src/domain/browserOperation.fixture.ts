import type { BrowserOperationProjection } from "./browserOperation";

export const browserOperationFixture = {
  projectId: "project-1",
  chatId: "chat-1",
  sessionId: "session-1",
  turnId: "turn-1",
  tabId: "browser-7",
  operationId: "operation-1",
  operation: "act",
  phase: "completed",
  origin: "https://example.com",
  summary: "Click element",
  resultSummary: "Action completed",
  createdAt: 1_780_000_000_000,
  updatedAt: 1_780_000_001_000,
} satisfies BrowserOperationProjection;
