export interface BrowserOperationProjection {
  projectId: string;
  chatId: string;
  sessionId: string;
  turnId: string;
  tabId: string;
  operationId: string;
  operation:
    | "unsupported"
    | "openTab"
    | "navigate"
    | "snapshotPage"
    | "act"
    | "waitFor"
    | "screenshot"
    | "readConsole"
    | "readNetwork";
  phase: "started" | "completed" | "denied" | "failed" | "outcomeUnknown";
  origin: string;
  summary: string;
  resultSummary?: string;
  createdAt: number;
  updatedAt: number;
}
